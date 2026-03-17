mod server;
mod storage;

use clap::Parser;
use config::Config;
use serde::{Deserialize, Serialize};
use server::ServerNode;
use std::path::Path;
use std::sync::Arc;
use storage::StorageDB;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::signal;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(name = "fernq-server")]
#[command(about = "FernQ Server Daemon - Unix Socket control interface")]
#[command(version)]
struct Cli {
    /// 配置文件路径 (INI format)
    #[arg(short, long, default_value = "./config/config.ini")]
    config: String,

    /// 存储目录路径
    #[arg(short, long, default_value = "./")]
    storage: String,

    /// 开发模式 (使用 /tmp/fernq.sock)
    #[arg(long)]
    dev: bool,
}

#[derive(Debug, Deserialize)]
struct ServerConfig {
    host: String,
    port: u16,
}

// Unix Socket 通信协议
#[derive(Debug, Deserialize)]
struct Request {
    cmd: String,
    #[serde(default)]
    room_name: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    room_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct Response {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_writer(std::io::stdout)
        .init();

    let cli = Cli::parse();
    info!("FernQ Server starting...");
    info!("Config: {}", cli.config);
    info!("Storage: {}", cli.storage);

    // 1. 加载配置文件 (config 0.15+ API)
    let cfg = Config::builder()
        .add_source(config::File::with_name(&cli.config).required(true))
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))?;

    let server_cfg: ServerConfig = cfg
        .get("server")
        .map_err(|e| anyhow::anyhow!("Missing [server] section: {}", e))?;

    let bind_addr = format!("{}:{}", server_cfg.host, server_cfg.port);
    info!("TCP bind address: {}", bind_addr);

    // 2. 初始化存储和节点
    let storage = Arc::new(StorageDB::new(&cli.storage)?);
    let node = Arc::new(ServerNode::new(storage));

    // 3. 启动 TCP 服务
    node.clone().start(bind_addr.clone()).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    info!("TCP server started");

    // 4. 设置 Unix Socket
    let sock_path = if cli.dev {
        "/tmp/fernq.sock"
    } else {
        "/run/fernq/fernq.sock"
    };

    // 创建父目录
    if let Some(parent) = Path::new(sock_path).parent()
        && !parent.exists()
    {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create socket dir: {}", e))?;
    }

    // 清理旧 socket
    if Path::new(sock_path).exists() {
        tokio::fs::remove_file(sock_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to remove old socket: {}", e))?;
    }

    let listener = UnixListener::bind(sock_path)
        .map_err(|e| anyhow::anyhow!("Failed to bind unix socket: {}", e))?;
    info!("Unix socket listening on: {}", sock_path);

    // 设置权限 0775
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(sock_path, std::fs::Permissions::from_mode(0o775))?;
    }

    // 5. 优雅退出处理
    let node_clone = node.clone();
    let sock_clone = sock_path.to_string();
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("Shutdown signal received");
                node_clone.close().await;
                let _ = tokio::fs::remove_file(&sock_clone).await;
                info!("Server shutdown complete");
                std::process::exit(0);
            }
            Err(e) => error!("Failed to listen for ctrl-c: {}", e),
        }
    });

    // 6. 主循环：处理 CLI 请求
    info!("Server ready, waiting for commands");
    loop {
        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                let node = node.clone();
                let addr = bind_addr.clone();

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    match stream.read(&mut buf).await {
                        Ok(n) if n > 0 => {
                            buf.truncate(n);

                            let req: Request = match serde_json::from_slice(&buf) {
                                Ok(r) => r,
                                Err(e) => {
                                    let resp = Response {
                                        success: false,
                                        data: None,
                                        error: Some(format!("Invalid JSON: {}", e)),
                                    };
                                    let _ = send_response(&mut stream, resp).await;
                                    return;
                                }
                            };

                            let resp = handle_request(node, &addr, req).await;
                            let _ = send_response(&mut stream, resp).await;
                        }
                        Ok(_) => {} // 连接关闭
                        Err(e) => error!("Failed to read from socket: {}", e),
                    }
                });
            }
            Err(e) => error!("Failed to accept connection: {}", e),
        }
    }
}

async fn send_response(stream: &mut tokio::net::UnixStream, resp: Response) -> anyhow::Result<()> {
    let json = serde_json::to_vec(&resp)?;
    stream.write_all(&json).await?;
    stream.flush().await?;
    Ok(())
}

async fn handle_request(node: Arc<ServerNode>, addr: &str, req: Request) -> Response {
    match req.cmd.as_str() {
        "add" => {
            let name = req.room_name.filter(|s| !s.is_empty());
            let pwd = req.password;

            if name.is_none() {
                return Response {
                    success: false,
                    data: None,
                    error: Some("room_name is required".into()),
                };
            }
            if pwd.is_none() {
                return Response {
                    success: false,
                    data: None,
                    error: Some("password is required".into()),
                };
            }

            match node.add_room(name.unwrap(), pwd.unwrap()).await {
                Ok(()) => Response {
                    success: true,
                    data: Some(serde_json::json!("room added")),
                    error: None,
                },
                Err(e) => Response {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to add room: {}", e)),
                },
            }
        }

        "remove" => {
            let id_str = req.room_id.filter(|s| !s.is_empty());

            if id_str.is_none() {
                return Response {
                    success: false,
                    data: None,
                    error: Some("room_id is required".into()),
                };
            }

            let id = match uuid::Uuid::parse_str(&id_str.unwrap()) {
                Ok(u) => u,
                Err(e) => {
                    return Response {
                        success: false,
                        data: None,
                        error: Some(format!("Invalid UUID format: {}", e)),
                    };
                }
            };

            match node.remove_room(id).await {
                Ok(()) => Response {
                    success: true,
                    data: Some(serde_json::json!("room removed")),
                    error: None,
                },
                Err(e) => Response {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to remove room: {}", e)),
                },
            }
        }

        "list" => {
            let rooms = node.list_rooms().await;
            let list: Vec<_> = rooms
                .into_iter()
                .map(|(id, name, pwd)| {
                    serde_json::json!({
                        "id": id,
                        "name": name,
                        "password": pwd
                    })
                })
                .collect();

            Response {
                success: true,
                data: Some(serde_json::Value::Array(list)),
                error: None,
            }
        }

        "address" => Response {
            success: true,
            data: Some(serde_json::Value::String(addr.to_string())),
            error: None,
        },

        _ => Response {
            success: false,
            data: None,
            error: Some(format!("Unknown command: {}", req.cmd)),
        },
    }
}
