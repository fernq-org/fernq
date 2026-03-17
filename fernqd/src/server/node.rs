use std::sync::Arc;
use std::vec;
use tracing::{error, info};

use super::room::Room;
use crate::server::conn::TcpConn;
use crate::storage::{StorageDB, StorageResult};
use dashmap::DashMap;
use fernq_core::protocol;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::BufReader;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

///  Server  Node 服务节点
#[derive(Clone)]
pub struct ServerNode {
    pub rooms: DashMap<Uuid, Arc<Room>>, // 房间集合
    pub storage_db: Arc<StorageDB>,      // 存储数据库
    /// 协程管理器（类似 Go 的 WaitGroup）
    pub worker_set: Arc<Mutex<JoinSet<()>>>,
    /// 全局取消令牌（类似 Go 的 context.Context）
    pub cancel_token: CancellationToken,
    /// TCP监听器存储，用于后续close操作
    pub listener: Arc<Mutex<Option<Arc<TcpListener>>>>,
    /// 绑定地址存储
    pub bind_addr: Arc<Mutex<Option<SocketAddr>>>,
}

impl ServerNode {
    /// 创建服务节点
    pub fn new(storage_db: Arc<StorageDB>) -> Self {
        Self {
            rooms: DashMap::new(),
            storage_db,
            cancel_token: CancellationToken::new(),
            worker_set: Arc::new(Mutex::new(JoinSet::new())),
            listener: Arc::new(Mutex::new(None)),
            bind_addr: Arc::new(Mutex::new(None)),
        }
    }

    /// 启动服务节点
    pub async fn start(self: Arc<Self>, bind_addr: String) {
        // info!("start: &self.rooms = {:p}", &self.rooms);
        // 从磁盘中获取所有房间信息,创建并打开实例
        match self.storage_db.list_all_rooms() {
            Ok(storage_rooms) => {
                if !storage_rooms.is_empty() {
                    for room in storage_rooms {
                        let (room_id, room_name, _) = room.get_info();
                        let current_room =
                            Room::new(room_id, room_name, 32, Duration::from_secs(60), 5);
                        // 打开房间
                        let arc_room = current_room.open().await;
                        // 放入集合
                        self.rooms.insert(room_id, arc_room);
                    }
                }
            }
            Err(e) => {
                error!("Failed to load rooms: {}", e);
            }
        }
        // 解析地址
        let addr = match bind_addr.parse::<SocketAddr>() {
            Ok(a) => a,
            Err(e) => {
                error!("Invalid bind address '{}': {}", bind_addr, e);
                return;
            }
        };

        // 用 Arc::new 包装
        let node = self.clone();
        self.worker_set.lock().await.spawn(async move {
            node.listen(addr).await;
        });
    }

    /// 关闭监听函数
    pub async fn close(&self) {
        info!("Closing server listener...");

        // 1. 触发取消令牌，listen 循环会退出，不再接受新连接
        self.cancel_token.cancel();

        // 2. 关闭所有房间
        info!("Closing {} rooms...", self.rooms.len());
        for entry in self.rooms.iter() {
            let room_id = *entry.key();
            let room = entry.value().clone();
            info!("Closing room: {}", room_id);
            room.close().await
        }
        // 清空房间映射表
        self.rooms.clear();
        info!("All rooms closed");

        // 3. 等待所有任务完成（包括 listen 和正在处理的连接）
        let mut workers = self.worker_set.lock().await;
        while let Some(res) = workers.join_next().await {
            match res {
                Ok(()) => info!("Worker finished"),
                Err(e) => error!("Worker failed: {:?}", e),
            }
        }

        info!("Server node closed");
    }

    /// 添加房间
    pub async fn add_room(&self, room_name: String, room_pwd: String) -> StorageResult<()> {
        // info!("add_room: &self.rooms = {:p}", &self.rooms);
        if self.is_closed() {
            return Err("Server node is closed".into());
        }

        // 1. 检查磁盘是否已存在同名房间
        let existing = self.storage_db.find_rooms_by_name(&room_name)?;
        if !existing.is_empty() {
            return Err(format!("Room '{}' already exists", room_name).into());
        }

        // 2. 创建房间并持久化到磁盘
        let room_model = self.storage_db.create_room(&room_name, &room_pwd)?;
        let (room_id, _, _) = room_model.get_info();

        // 3. 创建并启动房间
        let room = Room::new(room_id, room_name, 32, Duration::from_secs(60), 5);
        let arc_room = room.open().await;

        // 4. 加入内存中的房间管理表
        self.rooms.insert(room_id, arc_room);

        info!("Room {} created and opened successfully", room_id);
        Ok(())
    }

    /// 移除房间
    pub async fn remove_room(&self, room_id: Uuid) -> StorageResult<()> {
        if self.is_closed() {
            return Err("Server node is closed".into());
        }

        // 1. 检查房间是否存在于磁盘
        match self.storage_db.room_exists(room_id) {
            Ok(exists) => {
                if !exists {
                    return Err(format!("Room '{}' not found", room_id).into());
                }
            }
            Err(e) => {
                return Err(format!("Failed to check room existence: {}", e).into());
            }
        }

        info!("Removing room: {}", room_id);

        // 2. 从内存中移除并关闭房间
        if let Some((_, arc_room)) = self.rooms.remove(&room_id) {
            arc_room.close().await;
            info!("Room {} closed gracefully", room_id);
        } else {
            info!("Room {} was not in memory", room_id);
        }

        // 3. 从磁盘删除
        self.storage_db.delete_room(room_id)?;

        Ok(())
    }

    /// 获取房间列表（id,name,password）
    pub async fn list_rooms(&self) -> Vec<(String, String, String)> {
        let mut rooms = Vec::new();
        match self.storage_db.list_all_rooms() {
            Ok(storage_rooms) => {
                if !storage_rooms.is_empty() {
                    for room in storage_rooms {
                        let (room_id, room_name, room_pass) = room.get_info();
                        rooms.push((room_id.to_string(), room_name, room_pass));
                    }
                }
            }
            Err(_) => {
                return rooms;
            }
        }
        rooms
    }

    /// 判断是否关闭
    fn is_closed(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// 监听服务
    async fn listen(self: Arc<Self>, addr: SocketAddr) {
        // 1. 绑定端口
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to bind to {}: {}", addr, e);
                return;
            }
        };
        info!("Server node listening on {}", addr);

        // 2. 包成 Arc
        let listener_arc = Arc::new(listener);

        // 3. 存储到结构体
        *self.listener.lock().await = Some(Arc::clone(&listener_arc));
        *self.bind_addr.lock().await = Some(addr);

        // 4. accept 循环（直接用本地的 listener_arc，不从 self.listener 取）
        loop {
            tokio::select! {
                accept_result = listener_arc.accept() => {
                    match accept_result {
                        Ok((stream, addr)) => {
                            info!("Accepted connection from {}", addr);
                            let node = self.clone();
                            self.worker_set.lock().await.spawn(async move {
                                node.add_connection(stream, addr).await;
                            });
                        }
                        Err(e) => {
                            error!("Accept error: {}", e);
                            match e.kind() {
                                std::io::ErrorKind::Other |
                                std::io::ErrorKind::Interrupted => {
                                    info!("Listener closed, accept loop terminating");
                                    break;
                                }
                                _ => continue,
                            }
                        }
                    }
                }
                _ = self.cancel_token.cancelled() => {
                    let addr_str = self.bind_addr.lock().await
                        .as_ref()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    info!("Cancellation requested, stopping listener on {}", addr_str);
                    break;
                }
            }
        }

        // 5. 清理
        info!("Accept loop terminated gracefully");
        self.listener.lock().await.take();
    }

    /// 异步函数：验证第一条消息，通过后移交
    async fn add_connection(&self, stream: TcpStream, addr: SocketAddr) {
        // info!("add_connection: &self.rooms = {:p}", &self.rooms);
        info!("新连接: {}，开始验证...", addr);
        let mut reader = BufReader::new(stream);

        // 1. 读取固定头部（带超时）
        let mut header = [0u8; protocol::FIXED_HEADER_LEN];
        if let Err(e) = timeout(Duration::from_secs(5), reader.read_exact(&mut header)).await {
            error!("{} 读取头部超时或失败: {}", addr, e);
            let _ = reader.into_inner().shutdown().await;
            return;
        }

        // 2. 解析 body 长度
        let (msgtype, flags, body_len) = match protocol::decode_header(&header) {
            Ok(h) => h,
            Err(e) => {
                error!("解析头部失败: {}", e);
                let _ = reader.into_inner().shutdown().await;
                return;
            }
        };

        // 查看类型和标签
        if msgtype == protocol::MessageType::Ping || msgtype == protocol::MessageType::Pong {
            // 忽略 Ping
            let _ = reader.into_inner().shutdown().await;
            return;
        }

        let (end_s, end_c) = (flags.end_stream(), flags.end_channel());
        if !end_s && !end_c {
            // 忽略中间帧
            let _ = reader.into_inner().shutdown().await;
            return;
        }

        // 安全检查
        if body_len > protocol::MAX_FRAME_SIZE {
            error!("帧过大: {} bytes > {}", body_len, protocol::MAX_FRAME_SIZE);
            let _ = reader.into_inner().shutdown().await;
            return;
        }

        // 3. 读取 body（带超时）
        let mut body = vec![0u8; body_len];
        if let Err(e) = timeout(Duration::from_secs(5), reader.read_exact(&mut body)).await {
            error!("{} 读取 body 超时或失败: {}", addr, e);
            let _ = reader.into_inner().shutdown().await;
            return;
        }

        // 4. 拼接完整帧
        let mut frame = Vec::with_capacity(protocol::FIXED_HEADER_LEN + body_len);
        frame.extend_from_slice(&header);
        frame.extend_from_slice(&body);

        // 5. 解析帧
        let (header_info, payload) = match protocol::parse_message(&frame) {
            Ok(r) => r,
            Err(e) => {
                error!("解析消息失败: {}", e);
                let _ = reader.into_inner().shutdown().await;
                return;
            }
        };

        let conn_name = header_info.source.clone();
        let chan_id = header_info.channel_id;

        // 6. 信息读取完毕，准备验证信息
        let mut stream = reader.into_inner();
        let (room_id, room_name, room_pass) = match protocol::parse_validate(payload) {
            Ok(v) => v,
            Err(e) => {
                error!("解析验证信息失败: {}", e);
                let response = match protocol::create_verify_response(
                    &conn_name,
                    &chan_id,
                    false,
                    "验证信息格式错误",
                ) {
                    Ok(r) => r,
                    Err(_) => {
                        let _ = stream.shutdown().await;
                        return;
                    }
                };
                let _ = timeout(Duration::from_secs(5), stream.write_all(&response)).await;
                let _ = stream.shutdown().await;
                return;
            }
        };
        // 检查这个房间是否存在
        match self.storage_db.room_exists(room_id) {
            Ok(exists) => {
                if !exists {
                    error!("房间不存在: {}", room_id);
                    let response = match protocol::create_verify_response(
                        &conn_name,
                        &chan_id,
                        false,
                        "房间不存在",
                    ) {
                        Ok(r) => r,
                        Err(_) => {
                            error!("创建验证响应失败");
                            let _ = stream.shutdown().await;
                            return;
                        }
                    };
                    let _ = timeout(Duration::from_secs(5), stream.write_all(&response)).await;
                    let _ = stream.shutdown().await;
                    return;
                }
            }
            Err(e) => {
                error!("检查房间是否存在失败: {}", e);
                let response =
                    match protocol::create_verify_response(&conn_name, &chan_id, false, "系统错误")
                    {
                        Ok(r) => r,
                        Err(_) => {
                            let _ = stream.shutdown().await;
                            return;
                        }
                    };
                let _ = timeout(Duration::from_secs(5), stream.write_all(&response)).await;
                let _ = stream.shutdown().await;
                return;
            }
        }
        // 获取房间
        let room = match self.storage_db.get_room(room_id) {
            Ok(Some(room)) => room,
            Ok(None) => {
                error!("房间不存在");
                let response = match protocol::create_verify_response(
                    &conn_name,
                    &chan_id,
                    false,
                    "房间不存在",
                ) {
                    Ok(r) => r,
                    Err(_) => {
                        let _ = stream.shutdown().await;
                        return;
                    }
                };
                let _ = timeout(Duration::from_secs(5), stream.write_all(&response)).await;
                let _ = stream.shutdown().await;
                return;
            }
            Err(e) => {
                error!("获取房间信息失败: {}", e);
                let response =
                    match protocol::create_verify_response(&conn_name, &chan_id, false, "系统错误")
                    {
                        Ok(r) => r,
                        Err(_) => {
                            let _ = stream.shutdown().await;
                            return;
                        }
                    };
                let _ = timeout(Duration::from_secs(5), stream.write_all(&response)).await;
                let _ = stream.shutdown().await;
                return;
            }
        };
        // 比较房间名字
        if room.name != room_name {
            error!("房间名字不匹配");
            let response = match protocol::create_verify_response(
                &conn_name,
                &chan_id,
                false,
                "房间名称不匹配",
            ) {
                Ok(r) => r,
                Err(_) => {
                    let _ = stream.shutdown().await;
                    return;
                }
            };
            let _ = timeout(Duration::from_secs(5), stream.write_all(&response)).await;
            let _ = stream.shutdown().await;
            return;
        }
        // 验证密码
        if !room.verify_password(&room_pass) {
            error!("密码错误");
            let response =
                match protocol::create_verify_response(&conn_name, &chan_id, false, "密码错误")
                {
                    Ok(r) => r,
                    Err(_) => {
                        let _ = stream.shutdown().await;
                        return;
                    }
                };
            let _ = timeout(Duration::from_secs(5), stream.write_all(&response)).await;
            let _ = stream.shutdown().await;
            return;
        }
        // 验证成功,生成并返回响应信息
        let response = match protocol::create_verify_response(
            &conn_name,
            &chan_id,
            true,
            "Welcome to FernQ",
        ) {
            Ok(r) => r,
            Err(e) => {
                error!("生成响应失败: {}", e);
                let _ = stream.shutdown().await;
                return;
            }
        };

        // 发送响应，带5秒超时
        if let Err(e) = timeout(Duration::from_secs(5), stream.write_all(&response)).await {
            error!("发送响应超时或失败: {}", e);
            let _ = stream.shutdown().await;
            return;
        }
        // 7. 响应完成，封装连接提交给room线程处理
        let conn = TcpConn::new(conn_name, addr, stream);
        // 从 rooms 获取 room
        if let Some(room_ref) = self.rooms.get(&room_id) {
            // room_ref 是 Ref<Uuid, Arc<Room>>，解引用后 clone Arc
            let room = Arc::clone(&*room_ref);
            // 移交连接
            room.add_conn(conn).await;
        } else {
            error!("房间 {} 不在内存中", room_id);
        }
    }
}
