use std::fmt;

/// Storage 错误类型
#[derive(Debug)]
pub enum StorageError {
    /// 数据库操作错误（fjall 底层错误）
    Fjall(fjall::Error),
    /// 序列化/反序列化错误（bincode）
    Serialization(String),
    /// 数据格式错误（如 UUID 解析失败等）
    #[allow(dead_code)]
    InvalidData(String),
    /// 其他错误
    Other(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Fjall(e) => write!(f, "database error: {}", e),
            StorageError::Serialization(msg) => write!(f, "serialization error: {}", msg),
            StorageError::InvalidData(msg) => write!(f, "invalid data: {}", msg),
            StorageError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StorageError::Fjall(e) => Some(e),
            _ => None,
        }
    }
}

// 从 fjall::Error 自动转换
impl From<fjall::Error> for StorageError {
    fn from(e: fjall::Error) -> Self {
        StorageError::Fjall(e)
    }
}

// 从 bincode 解码错误转换
impl From<bincode::error::DecodeError> for StorageError {
    fn from(e: bincode::error::DecodeError) -> Self {
        StorageError::Serialization(format!("decode failed: {}", e))
    }
}

// 从 bincode 编码错误转换（虽然很少用到，但为了完整性）
impl From<bincode::error::EncodeError> for StorageError {
    fn from(e: bincode::error::EncodeError) -> Self {
        StorageError::Serialization(format!("encode failed: {}", e))
    }
}

impl From<&str> for StorageError {
    fn from(s: &str) -> Self {
        StorageError::Other(s.to_string())
    }
}

impl From<String> for StorageError {
    fn from(s: String) -> Self {
        StorageError::Other(s)
    }
}

/// Storage 结果类型
pub type StorageResult<T> = Result<T, StorageError>;
