use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{
    TcpStream,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// 高并发 TCP 连接封装
///
/// 设计目标：
/// - 读写分离：读端无锁，写端互斥，避免读写相互阻塞
/// - 优雅关闭：支持多协程共享连接，确保关闭逻辑只执行一次
/// - 状态感知：业务协程可快速检测连接是否已关闭，静默处理
/// - 超时保险：所有阻塞操作设置 30 秒超时，防止网络异常导致协程泄漏
///
/// 线程安全：
/// - reader: 只能被 take 一次，移交给单个读协程，无锁
/// - writer: Arc<Mutex<>> 支持多协程并发写入（内部序列化）
/// - cancel_token: 克隆共享，用于通知所有协程退出
/// - closed: AtomicBool 确保 close() 逻辑只执行一次（Tokio 异步 Once 模式）
pub struct TcpConn {
    /// 连接唯一标识，用于日志追踪和连接管理
    id: Uuid,

    /// 连接名称，业务层标识（如"gateway-conn-1"）
    name: String,

    /// 对端网络地址
    addr: SocketAddr,

    /// 读端：TcpStream 分离后的读半部分
    ///
    /// 使用 Option 包装，确保只能被 take 一次（移交给 read_loop 协程）
    /// 读协程通过 select! 监听数据读取和 cancel_token 取消信号
    reader: Option<OwnedReadHalf>,

    /// 写端：Arc<Mutex<>> 包装，支持多业务协程并发写入
    ///
    /// Mutex 确保写入操作序列化，避免 TCP 包交错
    /// Arc 允许多个克隆句柄共享同一写端
    writer: Arc<Mutex<OwnedWriteHalf>>,

    /// 取消令牌：Tokio 官方推荐的协作式取消机制
    ///
    /// - close() 时调用 cancel() 通知所有监听协程
    /// - 读协程通过 select! 监听 cancelled() 立即退出
    /// - 写协程通过 is_closed() 或 write() 错误检测后静默退出
    cancel_token: CancellationToken,

    /// 关闭标记：Arc<AtomicBool> 确保跨克隆体共享关闭状态
    ///
    /// 使用 AtomicBool::swap 实现 Tokio 生态的"异步 Once"模式：
    /// - swap(true) 返回 false：当前协程是唯一执行者，继续关闭逻辑
    /// - swap(true) 返回 true：已有其他克隆句柄执行过，直接返回
    ///
    /// 注意：Tokio 没有内置 async Once 类型，这是官方推荐做法
    closed: Arc<AtomicBool>,

    /// 心跳待回应次数（多协程共享）
    /// 使用 Arc<AtomicU32> 确保多句柄克隆时共享同一计数器
    pending_heartbeats: Arc<AtomicU32>,

    /// 操作超时：所有阻塞性 IO 操作的安全保险
    ///
    /// 防止网络分区、对端死机或内核 TCP 栈异常导致的无限阻塞
    /// 默认 30 秒，确保资源最终可释放，避免协程泄漏
    timeout: Duration,
}

impl TcpConn {
    /// 创建新连接，自动分离读写
    ///
    /// # 参数
    /// - name: 连接名称，业务层标识
    /// - addr: 对端地址，用于日志和调试
    /// - stream: Tokio accept 返回的 TcpStream，会被 into_split 分离
    ///
    /// # 返回
    /// 初始化完成的 TcpConn，reader 待 take，writer 可共享克隆
    pub fn new(name: impl Into<String>, addr: SocketAddr, stream: TcpStream) -> Self {
        // into_split 分离读写，两者独立，内核 TCP 缓冲区读写并行
        let (reader, writer) = stream.into_split();

        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            addr,
            reader: Some(reader),
            writer: Arc::new(Mutex::new(writer)),
            cancel_token: CancellationToken::new(),
            closed: Arc::new(AtomicBool::new(false)),
            pending_heartbeats: Arc::new(AtomicU32::new(0)),
            timeout: Duration::from_secs(30),
        }
    }

    /// 获取连接唯一 ID
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// 获取连接名称
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// 获取对端地址
    #[allow(dead_code)]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// 获取读端（只能调用一次）
    ///
    /// 返回后内部变为 None，防止重复获取导致所有权混乱
    /// 通常由 read_loop 协程在启动时调用，之后独占 reader
    ///
    /// # 返回
    /// - Some(OwnedReadHalf): 首次调用，返回读端所有权
    /// - None: 已被 take，重复调用返回空
    pub fn take_reader(&mut self) -> Option<OwnedReadHalf> {
        self.reader.take()
    }

    /// 获取取消令牌克隆
    ///
    /// 用于 read_loop 协程的 select! 监听：
    /// ```rust
    /// select! {
    ///     result = reader.read(&mut buf) => { /* 处理数据 */ },
    ///     _ = cancel_token.cancelled() => { /* 收到关闭信号，退出 */ },
    /// }
    /// ```
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    /// 获取写端共享句柄
    ///
    /// 返回 Arc<Mutex<OwnedWriteHalf>> 克隆，可传递给多个业务协程
    /// 业务协程通过 .lock().await 获取锁后写入
    #[allow(dead_code)]
    pub fn get_writer(&self) -> Arc<Mutex<OwnedWriteHalf>> {
        self.writer.clone()
    }

    /// 检查连接是否已关闭（非阻塞）
    ///
    /// 基于 CancellationToken::is_cancelled() 实现，零开销
    /// 业务协程可在写入前快速检测，避免无效锁竞争
    pub fn is_closed(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// 优雅关闭连接（幂等，线程安全，带超时保险）
    ///
    /// 执行流程（确保只执行一次）：
    /// 1. CAS 操作：AtomicBool::swap 检查是否已关闭
    /// 2. 独占阶段：获取写锁（带 30 秒超时），确保当前写入完成
    /// 3. 通知阶段：cancel_token.cancel() 广播关闭信号
    ///    - 读协程：select! 感知 cancelled()，立即退出循环
    ///    - 等待写入的协程：获取锁后检查 is_closed()，静默失败
    /// 4. 关闭阶段：执行 shutdown（带 30 秒超时），发送 FIN 给对端
    ///
    /// 超时处理：
    /// - 获取锁超时：视为强制关闭，防止死锁导致资源泄漏
    /// - shutdown 超时：视为半关闭成功，TCP 栈会自动清理
    ///
    /// # 线程安全
    /// - 多协程同时调用：只有一个成功执行关闭逻辑，其他立即返回 Ok(())
    /// - 与 write 并发：write 获取锁前/后检查 is_closed()，避免半关闭写入
    ///
    /// # 返回
    /// - Ok(()): 关闭成功（或已被其他协程关闭）
    /// - Err(e): shutdown 系统调用失败（极少见，如 fd 已无效）
    pub async fn close(&self) -> std::io::Result<()> {
        // Step 1: CAS 确保只执行一次（Tokio 异步 Once 模式）
        // swap(true) 返回旧值：false 表示首次设置，true 表示已被设置
        if self.closed.swap(true, Ordering::SeqCst) {
            // 已有其他协程执行过关闭，直接返回成功（幂等）
            return Ok(());
        }

        // Step 2: 独占写锁，带超时保险
        // 防止极端情况下（如对端死机导致写入阻塞）无法获取锁
        let mut writer = match timeout(self.timeout, self.writer.lock()).await {
            Ok(guard) => guard,
            Err(_) => {
                // 超时后强制取消，确保后续协程能快速失败
                self.cancel_token.cancel();
                return Ok(()); // 视为成功，资源会在 drop 时回收
            }
        };

        // Step 3: 广播取消信号，通知所有监听协程立即退出
        // 读协程的 select! 会立即从 cancelled() 分支返回
        // 业务协程后续调用 write() 会检测到 is_closed() 返回 true
        self.cancel_token.cancel();

        // Step 4: 执行 TCP shutdown，带超时保险
        // 发送 FIN 让对端感知连接关闭（对端 read 返回 Ok(0)）
        match timeout(self.timeout, writer.shutdown()).await {
            Ok(result) => result,
            Err(_) => {
                // 超时视为成功，避免阻塞调用方
                // 连接已通过 cancel_token 标记为死亡，资源后续回收
                Ok(())
            }
        }
    }

    /// 线程安全的写入操作（带 30 秒超时保险）
    ///
    /// 双重检查锁定模式（Double-Checked Locking）：
    /// 1. 快速路径：检查 is_closed()，已关闭直接返回错误（无锁开销）
    /// 2. 获取锁：等待当前写入完成（带 30 秒超时，防止死锁）
    /// 3. 慢速路径：再次检查 is_closed()，防止获取锁期间连接被关闭
    /// 4. 执行写入：调用 write_all 发送数据（带 30 秒超时，防止无限阻塞）
    ///
    /// 超时处理：
    /// - 获取锁超时：返回 TimedOut 错误，避免协程无限等待
    /// - 写入超时：自动触发 cancel_token，标记连接为死亡状态
    ///
    /// # 错误处理
    /// 返回 BrokenPipe 错误表示连接已关闭，调用方应静默处理：
    /// ```rust
    /// if let Err(e) = conn.write(data).await {
    ///     if e.kind() == ErrorKind::BrokenPipe {
    ///         // 连接已关闭，静默退出协程
    ///         return;
    ///     }
    /// }
    /// ```
    ///
    /// # 并发
    /// 多协程并发调用时，Mutex 确保写入序列化，避免 TCP 包交错
    pub async fn write(&self, data: &[u8]) -> std::io::Result<()> {
        // 快速路径：已关闭直接返回，避免无效锁竞争
        if self.is_closed() {
            return Err(std::io::Error::new(
                ErrorKind::BrokenPipe,
                "connection closed",
            ));
        }

        // 获取写锁，带超时保险
        // 防止 close() 持有锁期间阻塞导致协程泄漏
        let mut writer = match timeout(self.timeout, self.writer.lock()).await {
            Ok(guard) => guard,
            Err(_) => {
                return Err(std::io::Error::new(
                    ErrorKind::TimedOut,
                    "acquire write lock timeout",
                ));
            }
        };

        // 慢速路径：获取锁后再次检查，防止在等锁期间连接被关闭
        // 这是必要的，因为 close() 可能在 .lock().await 期间执行
        if self.is_closed() {
            return Err(std::io::Error::new(
                ErrorKind::BrokenPipe,
                "connection closed",
            ));
        }

        // 执行实际写入，带超时保险
        // 防止网络异常导致 write_all 无限阻塞（如对端接收窗口为 0 且死机）
        match timeout(self.timeout, writer.write_all(data)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // 写入超时，标记连接为死亡，防止后续写入继续阻塞
                self.cancel_token.cancel();
                Err(std::io::Error::new(ErrorKind::TimedOut, "write timeout"))
            }
        }
    }

    /// 获取当前待回应心跳次数（非阻塞）
    pub fn pending_heartbeats(&self) -> u32 {
        self.pending_heartbeats.load(Ordering::SeqCst)
    }

    /// 清零重置待回应心跳次数（收到心跳回应时调用）
    pub fn reset_heartbeats(&self) {
        self.pending_heartbeats.store(0, Ordering::SeqCst);
    }

    /// 待回应心跳次数+1（发送心跳时调用）
    pub fn increment_heartbeats(&self) {
        self.pending_heartbeats.fetch_add(1, Ordering::SeqCst);
    }
}

impl Clone for TcpConn {
    /// 克隆连接句柄（用于多协程共享写入）
    ///
    /// 克隆后：
    /// - id/name/addr/timeout 复制，便于追踪
    /// - reader: 置为 None（读端不可克隆，已移交给 read_loop）
    /// - writer/cancel_token: Arc 克隆，共享同一对象
    /// - closed: Arc 克隆，共享关闭状态，确保任意句柄调用 close() 均幂等
    ///
    /// 注意：通常只有原始句柄或指定主控句柄应调用 close()
    /// 但设计上支持任意克隆句柄关闭均有效，先到先得
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            addr: self.addr,
            reader: None,
            writer: self.writer.clone(),
            cancel_token: self.cancel_token.clone(),
            closed: self.closed.clone(), // Arc 克隆，共享标记
            pending_heartbeats: self.pending_heartbeats.clone(), // Arc 克隆，共享计数
            timeout: self.timeout,
        }
    }
}
