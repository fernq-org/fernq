use bincode::config::standard;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{StorageError, StorageResult};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RoomModel {
    pub id: Uuid,
    pub name: String,
    pub password: String,
}

impl RoomModel {
    /// 创建新房间
    pub fn new(name: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            password: password.into(),
        }
    }

    /// 序列化为字节
    /// 注意：为了与错误处理统一，改为返回 Result，业务逻辑不变
    pub fn to_bytes(&self) -> StorageResult<Vec<u8>> {
        let bytes = bincode::serde::encode_to_vec(self, standard())
            .map_err(|e| StorageError::Serialization(format!("encode failed: {}", e)))?;
        Ok(bytes)
    }

    /// 从字节反序列化
    pub fn from_bytes(bytes: &[u8]) -> StorageResult<Self> {
        let (result, _) = bincode::serde::decode_from_slice(bytes, standard())?;
        Ok(result)
    }

    /// 获取键（Uuid -> 16字节）
    pub fn key(&self) -> [u8; 16] {
        *self.id.as_bytes()
    }

    /// 克隆输出 id 和 name（不转移所有权）
    pub fn get_info(&self) -> (Uuid, String, String) {
        (self.id, self.name.clone(), self.password.clone())
    }

    /// 验证密码是否匹配
    pub fn verify_password(&self, input: &str) -> bool {
        self.password == input
    }
}
