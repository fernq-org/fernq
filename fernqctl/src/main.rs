use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::exit;

#[derive(Parser)]
#[command(name = "fernq-cli")]
#[command(about = "FernQ Server CLI Control Tool")]
#[command(version)]
struct Cli {
    /// Unix socket 路径
    #[arg(short, long, global = true)]
    socket: Option<String>,

    /// 开发模式 (使用 /tmp/fernq.sock)
    #[arg(long, global = true)]
    dev: bool,

    /// 输出原始 JSON 格式
    #[arg(long, global = true)]
    json: bool,

    /// 显示密码明文（默认隐藏）
    #[arg(long = "show_pwd", global = true)]
    show_pwd: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 添加新房间
    Add {
        /// 房间名称
        #[arg(short, long)]
        name: String,

        /// 房间密码
        #[arg(short, long)]
        password: String,
    },

    /// 删除房间 (通过 UUID)
    Remove {
        /// 房间 ID (UUID 格式)
        #[arg(short, long)]
        id: String,
    },

    /// 列出所有房间
    List,

    /// 获取服务器监听地址
    Address,

    /// 卸载 FernQ（需要 root 权限）
    Uninstall,
}

#[derive(Serialize)]
struct Request {
    cmd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    room_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    room_id: Option<String>,
}

#[derive(Deserialize, Serialize, Debug)]
struct Response {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red().bold(), e);
        exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // 确定 socket 路径
    let socket_path = cli.socket.unwrap_or_else(|| {
        if cli.dev {
            "/tmp/fernq.sock".to_string()
        } else {
            "/run/fernq/fernq.sock".to_string()
        }
    });

    // 构建请求
    let request = match &cli.command {
        Commands::Add { name, password } => Request {
            cmd: "add".to_string(),
            room_name: Some(name.clone()),
            password: Some(password.clone()),
            room_id: None,
        },
        Commands::Remove { id } => {
            // 预验证 UUID 格式
            if uuid::Uuid::parse_str(id).is_err() {
                anyhow::bail!("Invalid UUID format: {}", id);
            }
            Request {
                cmd: "remove".to_string(),
                room_name: None,
                password: None,
                room_id: Some(id.clone()),
            }
        }
        Commands::List => Request {
            cmd: "list".to_string(),
            room_name: None,
            password: None,
            room_id: None,
        },
        Commands::Address => Request {
            cmd: "address".to_string(),
            room_name: None,
            password: None,
            room_id: None,
        },
        Commands::Uninstall => {
            uninstall()?;
            return Ok(());
        }
    };

    // 发送请求
    let response = send_request(&socket_path, request)
        .with_context(|| format!("Cannot connect to fernqd at {}", socket_path))?;

    // 输出处理
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }

    if !response.success {
        let err = response
            .error
            .unwrap_or_else(|| "Unknown error".to_string());
        anyhow::bail!("Server error: {}", err);
    }

    // 友好格式输出
    match &cli.command {
        Commands::List => print_room_list(response.data, cli.show_pwd),
        Commands::Address => print_address(response.data),
        Commands::Add { name, .. } => {
            println!(
                "{} Room '{}' added successfully",
                "✓".green().bold(),
                name.cyan()
            );
        }
        Commands::Remove { id } => {
            println!(
                "{} Room '{}' removed successfully",
                "✓".green().bold(),
                id.cyan()
            );
        }
        Commands::Uninstall => {
            uninstall()?;
            return Ok(());
        }
    }

    Ok(())
}

fn send_request(socket_path: &str, req: Request) -> Result<Response> {
    let mut stream = UnixStream::connect(socket_path).with_context(|| "Is fernqd running?")?;

    // 设置超时
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;

    // 发送
    let req_json = serde_json::to_vec(&req)?;
    stream.write_all(&req_json)?;
    stream.shutdown(std::net::Shutdown::Write)?;

    // 接收
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;

    if buf.is_empty() {
        anyhow::bail!("Empty response from server");
    }

    let resp: Response = serde_json::from_slice(&buf).with_context(|| "Invalid response format")?;

    Ok(resp)
}

fn print_room_list(data: Option<serde_json::Value>, show_password: bool) {
    match data {
        Some(serde_json::Value::Array(rooms)) => {
            if rooms.is_empty() {
                println!("No rooms found.");
                return;
            }

            // 表头
            println!(
                "{:<36} {:<20} {}",
                "Room ID".bold().underline(),
                "Name".bold().underline(),
                "Password".bold().underline()
            );
            println!("{:-<36} {:-<20} {:-<16}", "", "", "");

            for room in &rooms {
                let id = room["id"].as_str().unwrap_or("N/A");
                let name = room["name"].as_str().unwrap_or("N/A");
                let pwd = room["password"].as_str().unwrap_or("N/A");

                let pwd_display = if show_password {
                    pwd.to_string()
                } else {
                    "******".to_string()
                };

                println!("{} {:<20} {}", id.dimmed(), name, pwd_display.yellow());
            }

            println!("\nTotal: {} room(s)", rooms.len().to_string().green());
        }
        Some(other) => println!("Unexpected response: {}", other),
        None => println!("No rooms found."),
    }
}

fn print_address(data: Option<serde_json::Value>) {
    match data {
        Some(serde_json::Value::String(addr)) => {
            println!("Server listening on: {}", addr.cyan().bold());
        }
        Some(other) => println!("Server address: {}", other),
        None => println!("Address not available"),
    }
}

fn uninstall() -> Result<()> {
    use std::process::Command;

    // 检查 root 权限
    let output = Command::new("id")
        .arg("-u")
        .output()
        .context("Failed to check user ID")?;
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if uid != "0" {
        anyhow::bail!("Please run: sudo fernq uninstall");
    }

    const SVC: &str = "fernq";

    println!("{} Stopping {} service...", "→".blue(), SVC);
    let _ = Command::new("systemctl").args(["stop", SVC]).output();

    println!("{} Disabling {} service...", "→".blue(), SVC);
    let _ = Command::new("systemctl").args(["disable", SVC]).output();

    println!("{} Removing service file...", "→".blue());
    let _ = std::fs::remove_file(format!("/etc/systemd/system/{}.service", SVC));

    println!("{} Reloading systemd daemon...", "→".blue());
    let _ = Command::new("systemctl").arg("daemon-reload").output();

    println!("{} Removing binaries...", "→".blue());
    let _ = std::fs::remove_file("/usr/local/bin/fernqd");
    let _ = std::fs::remove_file("/usr/local/bin/fernq");

    println!("{} Removing configuration...", "→".blue());
    let _ = std::fs::remove_dir_all("/etc/fernq");

    println!("{} Removing data directory...", "→".blue());
    let _ = std::fs::remove_dir_all("/var/lib/fernq");

    println!(
        "\n{} FernQ has been completely uninstalled.",
        "✓".green().bold()
    );

    Ok(())
}
