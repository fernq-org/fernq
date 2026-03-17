use super::conn::TcpConn;
use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use fernq_core::protocol;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use uuid::Uuid;

/// 任务结构体
pub struct MessageTask {
    pub from: String,
    pub to: String,
    pub flags: protocol::MessageFlags,
    pub payload: Bytes,
}

impl MessageTask {
    pub fn new(from: String, to: String, flags: protocol::MessageFlags, payload: Bytes) -> Self {
        Self {
            from,
            to,
            flags,
            payload,
        }
    }
}

/// 房间（Room）结构体
pub struct Room {
    #[allow(dead_code)]
    pub id: Uuid, // 房间唯一标识
    pub name: String,                         // 房间名称
    pub conns: DashMap<String, Arc<TcpConn>>, // 房间连接集合
    pub worker_number: usize,                 // 工作协程数量, 默认32
    pub heartbeat_interval: Duration,         // 心跳间隔
    pub max_pending_heartbeats: u32,          // 最大待回应心跳次数

    /// 通道发送端数组（用于 dispatch 发送任务）
    pub worker_senders: Vec<mpsc::Sender<MessageTask>>,

    /// 协程管理器（类似 Go 的 WaitGroup）
    pub worker_set: Arc<Mutex<JoinSet<()>>>,

    /// 全局取消令牌（类似 Go 的 context.Context）
    pub cancel_token: CancellationToken,

    /// 关闭标志（防止关闭期间添加新连接）
    pub is_closed: AtomicBool,
}

impl Room {
    /// 创建房间（仅初始化，未启动协程）
    pub fn new(
        id: Uuid,
        name: String,
        worker_number: usize,
        heartbeat_interval: Duration,
        max_pending_heartbeats: u32,
    ) -> Self {
        Self {
            id,
            name,
            conns: DashMap::new(),
            worker_number,
            heartbeat_interval,
            max_pending_heartbeats,
            worker_senders: Vec::with_capacity(worker_number),
            worker_set: Arc::new(Mutex::new(JoinSet::new())),
            cancel_token: CancellationToken::new(),
            is_closed: AtomicBool::new(false),
        }
    }

    /// 打开房间：创建通道、启动心跳和所有 Worker 协程
    pub async fn open(mut self) -> Arc<Self> {
        let mut receivers = Vec::with_capacity(self.worker_number);

        // 创建 worker_number 个通道，发送端保留，接收端收集起来
        for _ in 0..self.worker_number {
            let (tx, rx) = mpsc::channel::<MessageTask>(1024);
            self.worker_senders.push(tx);
            receivers.push(rx);
        }

        // 将 self 包进 Arc，供协程共享
        let arc = Arc::new(self);

        // 获取 JoinSet 的锁，用于 spawn 协程
        let mut set = arc.worker_set.lock().await;

        // 启动心跳协程
        let arc_hb = arc.clone();
        set.spawn(async move {
            arc_hb.heartbeat().await;
        });

        // 启动所有 Worker 协程，每个分配一个接收端
        for rx in receivers {
            let arc_wk = arc.clone();
            set.spawn(async move {
                arc_wk.worker(rx).await;
            });
        }

        // 释放锁（显式 drop 可选，函数结束会自动释放）
        drop(set);

        arc
    }

    /// 添加连接
    ///
    /// 注意：必须先调用 `open()` 函数将 Room 转换为 Arc<Room> 打开状态后，才能调用此方法
    /// 用法：room.clone().add_conn(conn).await;
    pub async fn add_conn(self: Arc<Self>, conn: TcpConn) {
        // 快速拒绝已关闭的房间
        if self.is_closed.load(Ordering::SeqCst) {
            // 直接关闭传入的连接，不启动协程
            if let Err(e) = conn.close().await {
                error!("关闭新连接失败（房间已关闭）: {}", e);
            }
            return;
        }

        let room = self.clone();
        let mut set = self.worker_set.lock().await;

        // 双重检查（防止在获取锁期间被关闭）
        if self.is_closed.load(Ordering::SeqCst) {
            drop(set); // 释放锁
            if let Err(e) = conn.close().await {
                error!("关闭新连接失败（房间已关闭）: {}", e);
            }
            return;
        }

        set.spawn(async move {
            room.listener(conn).await;
        });
    }

    /// 关闭房间：关闭所有连接、取消所有协程
    ///
    /// 注意：必须先调用 `open()` 函数将 Room 转换为 Arc<Room> 打开状态后，才能调用此方法
    /// 调用后会消耗 Arc<Room>，确保资源被正确释放
    pub async fn close(self: Arc<Self>) {
        info!("开始关闭房间: {}", self.name);

        // 1. 先设置关闭标志，阻止新连接进入
        self.is_closed.store(true, Ordering::SeqCst);

        // 2. 触发全局取消令牌，通知所有协程（listener、heartbeat、worker）开始退出
        self.cancel_token.cancel();

        // 3. 等待所有协程完成退出
        // listener 协程收到取消信号后会自动关闭连接并从 conns 中移除
        let mut set = self.worker_set.lock().await;
        while let Some(result) = set.join_next().await {
            if let Err(e) = result {
                error!("协程异常退出: {:?}", e);
            }
        }

        // 4. 防御性清理（理论上 conns 应该已经空了）
        self.conns.clear();

        info!("房间 {} 已完全关闭", self.name);
    }

    /// 检查room是否关闭
    async fn is_closed(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// 连接监听协程
    async fn listener(&self, mut conn: TcpConn) {
        if self.is_closed().await {
            // 房间已关闭,静默退出
            return;
        }
        let listener_id = conn.id(); // 获取连接唯一标识
        let listener_name = conn.name();
        // 1. 取出 reader 和 cancel_token
        let reader = match conn.take_reader() {
            Some(r) => r,
            None => return,
        };
        let conn_cancel = conn.cancel_token();
        let room_cancel = self.cancel_token.clone();

        // 2. 包进 Arc，后续所有操作通过 Arc 进行
        let conn = Arc::new(conn);

        // 3. 插入连接管理
        // 使用 entry API 原子性操作
        match self.conns.entry(conn.name()) {
            Entry::Occupied(mut entry) => {
                // 1. 原子替换：新连接立即插入，返回旧连接
                // insert 会消耗 entry，自动释放 shard 锁
                let old_conn = entry.insert(conn.clone());

                // 2. 在锁外异步关闭旧连接（不会阻塞其他线程访问 map）
                if let Err(e) = old_conn.close().await {
                    error!("关闭旧连接失败: {}", e);
                }
            }
            Entry::Vacant(entry) => {
                // 3. 无旧连接，直接插入
                entry.insert(conn.clone());
            }
        }
        info!("{} connected", listener_name);

        // 4. 创建带缓冲的读取器 + 动态缓冲区
        let mut buf_reader = BufReader::new(reader);

        // 5. 主循环：读取 -> 解析 -> 处理
        let mut header = [0u8; protocol::FIXED_HEADER_LEN]; // 长度固定，头部缓存复用
        loop {
            // 4.1 缓冲区数据不够，从网络读取更多
            tokio::select! {
                 // 连接的关闭信号
                 _ = conn_cancel.cancelled() => {
                                info!("连接取消");
                                break;
                            }
                // 房间关闭信号
                _ = room_cancel.cancelled() => {
                                info!("Room 关闭");
                                break;
                            }
                // 读取网络数据
                result = buf_reader.read_exact(&mut header) => {
                match result {
                    Ok(_) => {
                        // 4.2 解析头部获取帧长度
                        match protocol::decode_header(&header) {
                            Ok((msg_type, msg_flags, frame_length)) => {
                                // 安全检查：防止 frame_length 过大导致内存分配失败
                                if frame_length > protocol::MAX_FRAME_SIZE {
                                    error!("帧过大: {} bytes > {}, 连接: {}",
                                           frame_length, protocol::MAX_FRAME_SIZE, listener_name);
                                    break;
                                }

                                // 4.3 分配 payload 缓冲区
                                let mut payload = vec![0u8; frame_length];

                                // 关键点：读取 Payload 时也要监听取消信号！
                                tokio::select! {
                                    biased; // 优先检查取消信号，避免不必要的读取

                                    _ = conn_cancel.cancelled() => {
                                        info!("连接取消（读取payload阶段）");
                                        break;
                                    }
                                    _ = room_cancel.cancelled() => {
                                        info!("Room 关闭（读取payload阶段）");
                                        break;
                                    }

                                    result = buf_reader.read_exact(&mut payload) => {
                                        match result {
                                            Ok(_) => {
                                                // 4.4 处理完整帧（header + payload）
                                                if msg_type == protocol::MessageType::Pong {
                                                    // 4.4.0 处理 Pong, 刷新心跳计时器
                                                    conn.reset_heartbeats();
                                                    continue;
                                                }
                                                // 4.4.1 合并头部和负载
                                                let mut frame = BytesMut::with_capacity(header.len() + payload.len());
                                                frame.extend_from_slice(&header);
                                                frame.extend_from_slice(&payload);
                                                let frame = frame.freeze(); // 转为 Bytes

                                                // 4.4.2 解析帧，获取通道信息
                                                let channel_id = match protocol::get_channel_id(&frame) {
                                                    Ok(id) => id,
                                                    Err(e) => {
                                                        error!("获取通道ID失败: {:?}, 帧: {:?}", e, frame);
                                                        break;
                                                    }
                                                };
                                                // 4.4.3 获取信息来源和目标
                                                let (source_name, target_name) = match protocol::get_message_source_target(&frame) {
                                                    Ok(names) => names,
                                                    Err(e) => {
                                                        error!("获取信息来源和目标失败: {:?}, 帧: {:?}", e, frame);
                                                        break;
                                                    }
                                                };
                                                // 4.4.4哈希计算，选择处理协程
                                                if let Ok(worker_id) = channel_id.assign_target_fast(self.worker_number) {
                                                    if let Err(e) = self.worker_senders[worker_id]
                                                        .send(MessageTask::new(source_name, target_name, msg_flags, frame))
                                                        .await
                                                    {
                                                        error!("发送任务失败: {}", e);
                                                    }
                                                } else {
                                                    error!("无效的 channel_id，跳过消息");
                                                }
                                                // 4.4.5 刷新心跳计时器
                                                conn.reset_heartbeats();
                                            }
                                            Err(e) => {
                                                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                                                    info!("{} 连接在读取payload时关闭", listener_name);
                                                } else {
                                                    error!("读取 payload 失败: {}, 连接: {}", e, listener_name);
                                                }
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("协议头解析错误: {:?}, 连接: {}", e, listener_name);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::UnexpectedEof {
                            info!("{} 连接正常关闭", listener_name);
                        } else {
                            error!("读取头部失败: {}, 连接: {}", e, listener_name);
                        }
                        break;
                    }
                }
            }}
        }

        // 6. 清理逻辑：
        // 6.1 防御性删除：只有 ID 匹配才删（防止误删新连接）
        // 不需要处理返回值，因为 close() 已经确保资源释放
        self.conns
            .remove_if(&listener_name, |_, existing| existing.id() == listener_id);

        // 6.2 总是关闭当前连接（广播给所有 Arc 克隆体）
        if let Err(e) = conn.close().await {
            error!("关闭连接失败: {}", e);
        }

        info!("{} {} closed", listener_name, listener_id);
    }

    /// 心跳轮询发送
    async fn heartbeat(&self) {
        let ping = protocol::ping_frame();
        let mut interval = tokio::time::interval(self.heartbeat_interval);

        loop {
            tokio::select! {
                biased; // 优先检查取消信号

                _ = self.cancel_token.cancelled() => {
                    info!("心跳协程收到取消信号，退出");
                    break;
                }

                _ = interval.tick() => {
                    // 快照当前连接（避免在遍历过程中 DashMap 变更导致死锁）
                    let conns: Vec<Arc<TcpConn>> = self.conns
                        .iter()
                        .map(|e| e.value().clone())
                        .collect();

                    let max_pending = self.max_pending_heartbeats;

                    for conn in conns {
                        // 快速检查取消信号，确保关闭 Room 时立即中断
                        if self.cancel_token.is_cancelled() {
                            break;
                        }

                        let name = conn.name();
                        let pending = conn.pending_heartbeats();

                        // 1. 检查历史心跳是否已超时
                        if pending > max_pending {
                            error!("连接 {} 心跳超时 (pending: {} > {}), 执行关闭",
                                   name, pending, max_pending);
                            // close() 内部也有超时和幂等保护
                            let _ = conn.close().await;
                            continue;
                        }

                        // 2. 直接发送（依赖 conn.write 内部的30秒超时）
                        // 注意：如果连接数多且网络差，此处可能阻塞较长时间
                        match conn.write(&ping).await {
                            Ok(()) => {
                                conn.increment_heartbeats();
                            }
                            Err(e) => {
                                error!("向 {} 发送心跳失败: {}", name, e);
                                // 发送失败立即关闭（幂等，无需关心返回值）
                                let _ = conn.close().await;
                            }
                        }
                    }
                }
            }
        }
    }

    /// 处理信息协程
    async fn worker(&self, mut receiver: mpsc::Receiver<MessageTask>) {
        loop {
            tokio::select! {
                biased;

                _ = self.cancel_token.cancelled() => {
                    break;
                }

                maybe_task = receiver.recv() => {
                    let task = match maybe_task {
                        Some(t) => t,
                        None => break, // 所有发送端关闭，退出
                    };

                    // 查找目标连接，不存在则静默跳过
                    if let Some(entry) = self.conns.get(&task.to) {
                        let conn = entry.value().clone();
                        drop(entry); // 立即释放 DashMap 锁，避免阻塞其他操作

                        // 发送消息，失败静默处理
                        let _ = conn.write(&task.payload).await;
                        continue
                    }
                    // 未找到目标连接,同时是请求，除最后一帧外其余静默丢弃
                    if task.flags.end_stream() && task.flags.end_channel() && task.flags.is_request() {
                        // 最后一帧，响应没有该路径的错误
                        let Some(entry) = self.conns.get(&task.from) else {
                            continue;
                        };
                        let conn = entry.value().clone();
                        drop(entry); // 立即释放 DashMap 锁，避免阻塞其他操作
                        // 获取channel_id
                        let Ok(channel_id) = protocol::get_channel_id(&task.payload) else {
                            continue;
                        };
                        // 创建响应
                        let Ok(res) = protocol::create_message_response(&task.from, &channel_id, 404, "404 Not Found !")else {
                            continue;
                        };
                        // 发送消息，失败静默处理
                        let _ = conn.write(&res).await;
                    }
                }
            }
        }
    }
}
