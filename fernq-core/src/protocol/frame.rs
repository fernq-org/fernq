use super::error::{ProtocolError, ProtocolResult};
use super::{MAGIC, VERSION};
use uuid::Uuid;

/// 验证输入的4字节是否为合法魔数
pub fn validate_magic(bytes: &[u8]) -> ProtocolResult<u32> {
    if bytes.len() != 4 {
        return Err(ProtocolError::InvalidFrameLength(format!(
            "magic validation requires 4 bytes, got {}",
            bytes.len()
        )));
    }

    let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != MAGIC {
        return Err(ProtocolError::InvalidMagic {
            received: magic,
            expected: MAGIC,
        });
    }
    Ok(magic)
}

/// 验证版本号是否匹配
pub fn validate_version(byte: u8) -> ProtocolResult<u8> {
    if byte != VERSION {
        return Err(ProtocolError::InvalidVersion {
            received: byte,
            expected: VERSION,
        });
    }
    Ok(byte)
}

/// 消息类型定义（1字节）
///
/// 数值范围：0x01-0x04（保留 0x00 和 0x05-0xFF 用于未来扩展）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageType {
    /// 连接建立时的元数据交换（握手、认证信息）
    Header = 0x01,
    /// 业务数据载荷
    Body = 0x02,
    /// 心跳探测（保活检查）
    Ping = 0x03,
    /// 心跳响应
    Pong = 0x04,
}

impl MessageType {
    /// 编码为 u8（用于写入字节流）
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for MessageType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> ProtocolResult<Self> {
        match value {
            0x01 => Ok(Self::Header),
            0x02 => Ok(Self::Body),
            0x03 => Ok(Self::Ping),
            0x04 => Ok(Self::Pong),
            _ => Err(ProtocolError::UnknownMessageType(value)),
        }
    }
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Header => write!(f, "HEADER"),
            Self::Body => write!(f, "Body"),
            Self::Ping => write!(f, "PING"),
            Self::Pong => write!(f, "PONG"),
        }
    }
}

/// 消息标志位（1 字节）
///
/// 位布局（从高位向低位）：
/// - bit 7: END_STREAM (0x80) - 流结束标志
/// - bit 6: END_CHANNEL (0x40) - 通道结束标志  
/// - bit 5: IS_RESPONSE (0x20) - 请求/响应标志（0=请求, 1=响应）
/// - bit 4-0: 保留位（必须为 0）                             
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MessageFlags(u8);

impl MessageFlags {
    /// 位掩码常量
    pub const END_STREAM: u8 = 0b1000_0000;
    pub const END_CHANNEL: u8 = 0b0100_0000;
    pub const IS_RESPONSE: u8 = 0b0010_0000; // bit 5（新增：0=请求, 1=响应）
    pub const RESERVED_MASK: u8 = 0b0001_1111;

    /// 创建 flags（从三个 bool 参数）
    ///
    /// # Examples
    /// ```
    /// let flags = MessageFlags::new(true, false, true);
    /// assert_eq!(flags.as_u8(), 0xA0);
    /// assert!(flags.end_stream());
    /// assert!(!flags.end_channel());
    /// assert!(flags.is_response());
    /// ```
    pub const fn new(end_stream: bool, end_channel: bool, is_response: bool) -> Self {
        let mut val = 0u8;
        if end_stream {
            val |= Self::END_STREAM;
        }
        if end_channel {
            val |= Self::END_CHANNEL;
        }
        if is_response {
            val |= Self::IS_RESPONSE;
        }
        Self(val)
    }

    /// 编码为 u8（用于写入字节流）
    pub const fn as_u8(self) -> u8 {
        self.0
    }

    /// 获取 END_STREAM 状态
    pub const fn end_stream(&self) -> bool {
        (self.0 & Self::END_STREAM) != 0
    }

    /// 获取 END_CHANNEL 状态  
    pub const fn end_channel(&self) -> bool {
        (self.0 & Self::END_CHANNEL) != 0
    }

    /// 获取 IS_RESPONSE 状态
    pub const fn is_response(&self) -> bool {
        (self.0 & Self::IS_RESPONSE) != 0
    }

    /// 判断是否为请求
    pub const fn is_request(&self) -> bool {
        !self.is_response()
    }

    /// 检查保留位是否为 0（协议校验）
    pub const fn is_valid(&self) -> bool {
        (self.0 & Self::RESERVED_MASK) == 0
    }
}

impl TryFrom<u8> for MessageFlags {
    type Error = ProtocolError;

    fn try_from(value: u8) -> ProtocolResult<Self> {
        // 自动检查 bit 4-0 必须为 0（bit 5 现在合法了）
        if (value & Self::RESERVED_MASK) != 0 {
            return Err(ProtocolError::InvalidFlags(value));
        }
        Ok(Self(value))
    }
}

impl std::fmt::Display for MessageFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[END_STREAM={}, END_CHANNEL={}, IS_RESPONSE={}]",
            self.end_stream(),
            self.end_channel(),
            self.is_response()
        )
    }
}

impl From<MessageFlags> for (bool, bool, bool) {
    fn from(flags: MessageFlags) -> Self {
        (flags.end_stream(), flags.end_channel(), flags.is_response())
    }
}

/// 通道 ID 封装，基于 UUID v4（完全随机，128 位均匀分布）
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChannelId(Uuid);

impl ChannelId {
    /// 生成新的通道 ID（UUID v4，完全随机）
    #[inline]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// 从现有 UUID 构造
    #[inline]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// 获取内部 UUID 引用
    #[inline]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// 借用返回 16 字节切片（可多次调用，零拷贝）
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }

    /// 消耗性转换为 16 字节数组（用于最后转移所有权）
    #[inline]
    pub fn into_bytes(self) -> [u8; 16] {
        self.0.into_bytes()
    }

    /// 从 16 字节数组构造
    #[inline]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    /// 尝试从字节切片构造（长度必须为 16）
    #[inline]
    pub fn try_from_slice(slice: &[u8]) -> ProtocolResult<Self> {
        match slice.try_into() {
            Ok(bytes) => Ok(Self::from_bytes(bytes)),
            Err(_) => Err(ProtocolError::InvalidUuidLength {
                expected: 16,
                actual: slice.len(),
            }),
        }
    }

    /// 获取 UUID 后 8 字节作为 u64（用于一致性哈希）
    #[inline]
    pub fn hash_u64(&self) -> u64 {
        let b = self.0.as_bytes();
        u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]])
    }

    /// 标准分配：后 8 字节对总数取模（支持任意正整数）
    ///
    /// 使用除法运算，适用于任意目标数量。
    /// 性能：约 20-30 CPU 周期（除法指令）
    #[inline]
    pub fn assign_target(&self, total_targets: usize) -> ProtocolResult<usize> {
        if total_targets == 0 {
            return Err(ProtocolError::InvalidTargetCount {
                received: 0,
                reason: "must be greater than 0".to_string(),
            });
        }
        Ok((self.hash_u64() as usize) % total_targets)
    }

    /// 超快分配：位运算版本（目标数必须是 2 的幂）
    ///
    /// 利用位掩码替代取模：hash & (n-1) == hash % n（当 n 是 2 的幂时）
    ///
    /// # 要求
    /// - `total_targets` 必须是 2 的幂（如 16, 32, 64, 128, 256）
    /// - 推荐使用 16 或 32（平衡并发度与单节点负载）
    ///
    /// # 性能
    /// - 仅 1 个 CPU 周期（位与运算）
    /// - 比标准取模快 20-30 倍
    ///
    /// # 示例
    /// ```
    /// let id = ChannelId::new();
    /// let target = id.assign_target_fast(32)?; // 0-31 之间，O(1) 极快
    /// ```
    #[inline]
    pub fn assign_target_fast(&self, total_targets: usize) -> ProtocolResult<usize> {
        if !total_targets.is_power_of_two() {
            return Err(ProtocolError::InvalidTargetCount {
                received: total_targets,
                reason: "must be power of 2 for fast path (e.g., 16, 32, 64, 128)".to_string(),
            });
        }
        // 位与运算：x % 2^n == x & (2^n - 1)
        Ok((self.hash_u64() as usize) & (total_targets - 1))
    }
}

impl Default for ChannelId {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<[u8; 16]> for ChannelId {
    type Error = ProtocolError;

    #[inline]
    fn try_from(bytes: [u8; 16]) -> ProtocolResult<Self> {
        Ok(Self::from_bytes(bytes))
    }
}

impl TryFrom<&[u8]> for ChannelId {
    type Error = ProtocolError;

    #[inline]
    fn try_from(slice: &[u8]) -> ProtocolResult<Self> {
        Self::try_from_slice(slice)
    }
}

impl From<ChannelId> for [u8; 16] {
    #[inline]
    fn from(id: ChannelId) -> Self {
        id.into_bytes()
    }
}

#[derive(Debug)]
pub struct MessageHeader {
    pub channel_id: ChannelId,
    pub source: String,
    pub target: String,
    pub total_len: usize,
    pub stream_offset: usize,
}
