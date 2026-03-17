use std::fmt;

/// 协议错误类型
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolError {
    /// 未知的消息类型
    UnknownMessageType(u8),
    /// 非法的标志位（保留位不为0）
    InvalidFlags(u8),
    /// 魔数不匹配
    InvalidMagic {
        received: u32,
        expected: u32,
    },
    /// 版本号不匹配
    InvalidVersion {
        received: u8,
        expected: u8,
    },
    /// 帧长度非法（通用格式错误）
    InvalidFrameLength(String),
    /// 帧数据不完整（长度不足）- 用于两次长度检查
    IncompleteFrame {
        /// 需要的最小字节数
        required: usize,
        /// 实际可用的字节数
        actual: usize,
        /// 错误上下文（如 "fixed header", "complete frame"）
        context: &'static str,
    },
    /// 名称长度超过限制
    NameTooLong {
        name_type: &'static str,
        length: usize,
        max: u16,
    },
    /// 数据流长度超过协议限制（关键：防止 usize -> u32 截断）
    StreamTooLong {
        length: usize,
        max: u32,
    },
    /// 数值计算溢出（防御性：usize 运算中的算术溢出）
    LengthOverflow(&'static str),
    /// 数据流偏移超出范围
    InvalidOffset {
        offset: u32,
        total: u32,
    },
    /// CRC校验失败
    CrcMismatch {
        received: u16,
        calculated: u16,
    },
    /// 帧格式错误
    MalformedFrame(String),
    /// 空名称不允许
    EmptyName(&'static str),
    /// UUID 字节长度无效（必须为 16 字节）
    InvalidUuidLength {
        expected: usize,
        actual: usize,
    },
    /// UUID 格式无效（字节数组无法解析为有效 UUID）
    InvalidUuidFormat(String),
    /// 目标数量无效（用于分配函数）
    InvalidTargetCount {
        received: usize,
        reason: String,
    },
    // 名字错误
    InvalidName(&'static str),
    /// 无效的协议（不以fernq://开头）
    InvalidProtocol(String),
    /// 缺少地址
    EmptyAddress,
    /// 无效的端口号
    InvalidPort(String),
    /// Payload 不是有效的 UTF-8 编码
    InvalidUtf8,
    /// URL 中缺少 UUID
    MissingUuid,
    /// URL 中缺少房间名称（缺少 #fragment）
    MissingName,
    /// URL 中缺少密码（缺少 room_pass 参数）
    MissingPassword,
    /// 密码值为空（room_pass= 后面没有内容）
    EmptyPassword,
    /// JSON 解析或格式错误
    InvalidJson(String),
    /// 无效的 Header 类型（不是 request/response）
    InvalidHeaderType(String),
    /// 无效的 Content 类型
    InvalidContentType(String),
    /// 缺少必需的 Path 字段（Request 类型必需）
    MissingPath,
    /// 缺少必需的 State 字段（Response 类型必需）
    MissingState,
    /// 意外的 Body 字段（Form/FormFlow 类型不应有 body）
    UnexpectedBody,
    /// 缺少必需的 Body 字段（已废弃，Json 现在 body 可选）
    #[deprecated(note = "Json content type now allows optional body")]
    MissingBody,
    /// 无效的状态码
    InvalidStateCode(i64),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::UnknownMessageType(v) => {
                write!(f, "unknown message type: 0x{:02X}", v)
            }
            ProtocolError::InvalidFlags(v) => {
                write!(f, "invalid flags: 0x{:02X} (reserved bits must be zero)", v)
            }
            ProtocolError::InvalidMagic { received, expected } => {
                write!(
                    f,
                    "magic mismatch: received 0x{:08X}, expected 0x{:08X}",
                    received, expected
                )
            }
            ProtocolError::InvalidVersion { received, expected } => {
                write!(
                    f,
                    "version mismatch: received {}, expected {}",
                    received, expected
                )
            }
            ProtocolError::InvalidFrameLength(msg) => {
                write!(f, "invalid frame length: {}", msg)
            }
            ProtocolError::IncompleteFrame {
                required,
                actual,
                context,
            } => {
                write!(
                    f,
                    "incomplete frame ({}): required {} bytes, got {}",
                    context, required, actual
                )
            }
            ProtocolError::NameTooLong {
                name_type,
                length,
                max,
            } => {
                write!(
                    f,
                    "{} name too long: {} bytes (max: {})",
                    name_type, length, max
                )
            }
            ProtocolError::StreamTooLong { length, max } => {
                write!(f, "stream too long: {} bytes (max: {} bytes)", length, max)
            }
            ProtocolError::LengthOverflow(context) => {
                write!(f, "arithmetic overflow in: {}", context)
            }
            ProtocolError::InvalidOffset { offset, total } => {
                write!(f, "invalid offset: {} (total: {})", offset, total)
            }
            ProtocolError::CrcMismatch {
                received,
                calculated,
            } => {
                write!(
                    f,
                    "CRC mismatch: received 0x{:04X}, calculated 0x{:04X}",
                    received, calculated
                )
            }
            ProtocolError::MalformedFrame(msg) => {
                write!(f, "malformed frame: {}", msg)
            }
            ProtocolError::EmptyName(name_type) => {
                write!(f, "{} name cannot be empty", name_type)
            }
            ProtocolError::InvalidUuidLength { expected, actual } => {
                write!(
                    f,
                    "invalid UUID length: expected {} bytes, got {}",
                    expected, actual
                )
            }
            ProtocolError::InvalidUuidFormat(msg) => {
                write!(f, "invalid UUID format: {}", msg)
            }
            ProtocolError::InvalidTargetCount { received, reason } => {
                write!(f, "invalid target count: {} ({})", received, reason)
            }
            ProtocolError::InvalidName(name_type) => {
                write!(f, "{} name cannot be empty", name_type)
            }
            ProtocolError::InvalidProtocol(url) => {
                write!(f, "invalid protocol scheme: {} (expected fernq://)", url)
            }
            ProtocolError::EmptyAddress => {
                write!(f, "address is empty after fernq://")
            }
            ProtocolError::InvalidPort(port) => {
                write!(f, "invalid port number: {}", port)
            }
            ProtocolError::InvalidUtf8 => {
                write!(f, "payload is not valid UTF-8")
            }
            ProtocolError::MissingUuid => {
                write!(f, "missing UUID in URL path")
            }
            ProtocolError::MissingName => {
                write!(f, "missing room name (expected #name)")
            }
            ProtocolError::MissingPassword => {
                write!(f, "missing room_pass in query string")
            }
            ProtocolError::EmptyPassword => {
                write!(f, "room_pass cannot be empty")
            }
            ProtocolError::InvalidJson(msg) => {
                write!(f, "json parse error: {}", msg)
            }
            ProtocolError::InvalidHeaderType(ty) => {
                write!(
                    f,
                    "invalid header_type: {} (expected 'request' or 'response')",
                    ty
                )
            }
            // 修改：更新错误提示，移除 'string'
            ProtocolError::InvalidContentType(ty) => {
                write!(
                    f,
                    "invalid content_type: {} (expected 'json', 'form', or 'form_flow')",
                    ty
                )
            }
            ProtocolError::MissingPath => {
                write!(f, "missing required field 'path' for request")
            }
            ProtocolError::MissingState => {
                write!(f, "missing required field 'state' for response")
            }
            ProtocolError::UnexpectedBody => {
                write!(f, "unexpected body field for form/form_flow content type")
            }
            #[allow(deprecated)]
            ProtocolError::MissingBody => {
                write!(
                    f,
                    "missing required body field for json content type (deprecated: body is now optional)"
                )
            }
            ProtocolError::InvalidStateCode(code) => {
                write!(
                    f,
                    "invalid state code: {} (expected HTTP status code)",
                    code
                )
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

/// 协议结果类型
pub type ProtocolResult<T> = Result<T, ProtocolError>;
