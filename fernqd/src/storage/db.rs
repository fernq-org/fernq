use fjall::{Database, Keyspace, KeyspaceCreateOptions, PersistMode};
use std::path::Path;

use super::error::StorageResult;
use super::model::RoomModel;
use uuid::Uuid;

const KEYSPACE_ROOMS: &str = "rooms";

/// 数据库存储
pub struct StorageDB {
    db: Database,
    rooms: Keyspace,
}

impl StorageDB {
    /// 创建数据库实例,路径存在时，将打开数据库，否则创建数据库
    /// 注：为了统一，改为 StorageResult，如要保持原样可改回 Result<Self, fjall::Error>
    pub fn new(db_path: impl AsRef<Path>) -> StorageResult<Self> {
        let path = db_path.as_ref();

        let db = Database::builder(path).open()?;
        let rooms = db.keyspace(KEYSPACE_ROOMS, KeyspaceCreateOptions::default)?;

        Ok(Self { db, rooms })
    }

    /// 创建房间
    pub fn create_room(
        &self,
        name: impl Into<String>,
        password: impl Into<String>,
    ) -> StorageResult<RoomModel> {
        let room = RoomModel::new(name, password);
        self.save_room(&room)?;
        Ok(room)
    }

    /// 保存房间
    pub fn save_room(&self, room: &RoomModel) -> StorageResult<()> {
        self.rooms.insert(room.key(), room.to_bytes()?)?;
        self.db.persist(PersistMode::SyncAll)?;
        Ok(())
    }

    /// 获取房间（关键修改：改为 StorageResult）
    pub fn get_room(&self, id: Uuid) -> StorageResult<Option<RoomModel>> {
        match self.rooms.get(id.as_bytes())? {
            Some(bytes) => Ok(Some(RoomModel::from_bytes(&bytes)?)),
            None => Ok(None),
        }
    }

    /// 列出所有房间（关键修改：改为 StorageResult）
    pub fn list_all_rooms(&self) -> StorageResult<Vec<RoomModel>> {
        let mut rooms = Vec::new();
        for item in self.rooms.iter() {
            let value_bytes = item.value()?;
            rooms.push(RoomModel::from_bytes(&value_bytes)?);
        }
        Ok(rooms)
    }

    /// 根据名称查找房间（关键修改：改为 StorageResult）
    pub fn find_rooms_by_name(&self, name: &str) -> StorageResult<Vec<RoomModel>> {
        let mut result = Vec::new();
        for item in self.rooms.iter() {
            let value_bytes = item.value()?;
            let room = RoomModel::from_bytes(&value_bytes)?;
            if room.name == name {
                result.push(room);
            }
        }
        Ok(result)
    }

    /// 删除房间
    pub fn delete_room(&self, id: Uuid) -> StorageResult<bool> {
        let key = id.as_bytes();
        let existed = self.rooms.get(key)?.is_some();

        if existed {
            self.rooms.remove(key)?;
            self.db.persist(PersistMode::SyncAll)?;
        }

        Ok(existed)
    }

    /// 获取房间数量
    #[allow(dead_code)]
    pub fn count_rooms(&self) -> StorageResult<usize> {
        let count = self.rooms.len()?;
        Ok(count)
    }

    /// 房间是否存在
    pub fn room_exists(&self, id: Uuid) -> StorageResult<bool> {
        Ok(self.rooms.get(id.as_bytes())?.is_some())
    }
}
