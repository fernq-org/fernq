use super::error::{ProtocolError, ProtocolResult};
use super::frame::{
    ChannelId, MessageFlags, MessageHeader, MessageType, validate_magic, validate_version,
};

use super::{
    CHANNEL_ID_LEN, FIXED_HEADER_LEN, FLAGS_LEN, MAGIC_LEN, MAX_NAME_LENGTH, MESSAGE_FIXED_LEN,
    NET_CRC, SOURCE_LEN_SIZE, STREAM_OFFSET_LEN, TARGET_LEN_SIZE, TOTAL_STREAM_LEN, TYPE_LEN,
    VERSION_LEN,
};

/// 初级解码，只解码固定协议头
///
/// 验证魔数、版本号
/// 获取帧长度字段
pub fn decode_header(bytes: &[u8]) -> ProtocolResult<(MessageType, MessageFlags, usize)> {
    // 1. 检测帧长度，小于 MAGIC_LEN 报错
    if bytes.len() < MAGIC_LEN {
        return Err(ProtocolError::IncompleteFrame {
            required: MAGIC_LEN,
            actual: bytes.len(),
            context: "fixed header",
        });
    }
    // 2. 校验魔数（前4字节）
    let _magic = validate_magic(&bytes[0..MAGIC_LEN])?;
    // 3. 校验版本号（第5字节）
    let _version = validate_version(bytes[MAGIC_LEN])?;
    // 4. 获取类型
    let msg_type = MessageType::try_from(bytes[MAGIC_LEN + VERSION_LEN])?;
    // 5. 获取帧标志字段
    let flags = MessageFlags::try_from(bytes[MAGIC_LEN + VERSION_LEN + TYPE_LEN])?;
    // 6. 获取帧长度字段（4字节，大端序）
    let frame_length = u32::from_be_bytes([
        bytes[MAGIC_LEN + VERSION_LEN + TYPE_LEN + FLAGS_LEN],
        bytes[MAGIC_LEN + VERSION_LEN + TYPE_LEN + FLAGS_LEN + 1],
        bytes[MAGIC_LEN + VERSION_LEN + TYPE_LEN + FLAGS_LEN + 2],
        bytes[MAGIC_LEN + VERSION_LEN + TYPE_LEN + FLAGS_LEN + 3],
    ]) as usize;
    Ok((msg_type, flags, frame_length))
}

/// 初级解码，只解码固定协议头
///
/// 验证魔数、版本号
///
/// 返回：
///     - 消息类型 type
///     - 帧标志 flags
///     - 当前帧引用
///     - byte的剩余数据
pub fn decode_basic(bytes: &[u8]) -> ProtocolResult<(MessageType, MessageFlags, &[u8], &[u8])> {
    // 1. 第一次长度检查：是否达到固定头长度
    if bytes.len() < FIXED_HEADER_LEN {
        return Err(ProtocolError::IncompleteFrame {
            required: FIXED_HEADER_LEN,
            actual: bytes.len(),
            context: "fixed header",
        });
    }

    // 2. 校验魔数（前4字节）
    let _magic = validate_magic(&bytes[0..MAGIC_LEN])?;
    // 3. 获取帧长度字段（4字节，大端序）
    let frame_len_offset = MAGIC_LEN + VERSION_LEN + TYPE_LEN + FLAGS_LEN;
    let frame_length = u32::from_be_bytes([
        bytes[frame_len_offset],
        bytes[frame_len_offset + 1],
        bytes[frame_len_offset + 2],
        bytes[frame_len_offset + 3],
    ]) as usize;
    // 4. 计算总帧长度：固定头 + 帧体长度（frame_length 包含 Payload + CRC）
    let total_frame_len =
        FIXED_HEADER_LEN
            .checked_add(frame_length)
            .ok_or(ProtocolError::LengthOverflow(
                "total frame length calculation",
            ))?;
    // 5. 第二次长度检查：是否达到完整帧长度
    if bytes.len() < total_frame_len {
        return Err(ProtocolError::IncompleteFrame {
            required: total_frame_len,
            actual: bytes.len(),
            context: "complete frame",
        });
    }
    // 6. 分割当前帧和剩余数据
    let current_frame = &bytes[0..total_frame_len];
    let remaining = &bytes[total_frame_len..];
    // 7. CRC校验：最后两个字节是校验和
    let crc_offset = total_frame_len
        .checked_sub(2)
        .ok_or(ProtocolError::MalformedFrame(
            "frame too short for CRC".to_string(),
        ))?;
    // 校验范围：从 Version 字段开始到 CRC 之前（与生成端对应）
    let calculated_crc = NET_CRC.checksum(&current_frame[MAGIC_LEN..crc_offset]);
    let received_crc =
        u16::from_be_bytes([current_frame[crc_offset], current_frame[crc_offset + 1]]);
    if calculated_crc != received_crc {
        return Err(ProtocolError::CrcMismatch {
            received: received_crc,
            calculated: calculated_crc,
        });
    }
    // 8. 校验版本号（第4字节，Magic 之后）
    let version = bytes[MAGIC_LEN];
    validate_version(version)?;
    // 9. 解析消息类型和标志位
    let msg_type = MessageType::try_from(bytes[MAGIC_LEN + VERSION_LEN])?;
    let flags = MessageFlags::try_from(bytes[MAGIC_LEN + VERSION_LEN + TYPE_LEN])?;

    Ok((msg_type, flags, current_frame, remaining))
}

/// 获取当前帧的通道id
pub fn get_channel_id(bytes: &[u8]) -> ProtocolResult<ChannelId> {
    // 1. 检测帧长度，小于 MESSAGE_FIXED_LEN 报错
    if bytes.len() < MESSAGE_FIXED_LEN {
        return Err(ProtocolError::IncompleteFrame {
            required: MESSAGE_FIXED_LEN,
            actual: bytes.len(),
            context: "channel id",
        });
    }

    // 2. 提取通道 ID 字节范围 [FIXED_HEADER_LEN..FIXED_HEADER_LEN+CHANNEL_ID_LEN]
    let channel_id_start = FIXED_HEADER_LEN;
    let channel_id_end = FIXED_HEADER_LEN + CHANNEL_ID_LEN;

    // 3. 转换为 ChannelId
    ChannelId::try_from_slice(&bytes[channel_id_start..channel_id_end])
}

/// 获取信息来源和目标
pub fn get_message_source_target(bytes: &[u8]) -> ProtocolResult<(String, String)> {
    // 1. 检测帧长度，小于 MESSAGE_FIXED_LEN 报错
    if bytes.len() < MESSAGE_FIXED_LEN {
        return Err(ProtocolError::IncompleteFrame {
            required: MESSAGE_FIXED_LEN,
            actual: bytes.len(),
            context: "fixed header",
        });
    }
    // 获取信息来源和目标
    let mut offset = FIXED_HEADER_LEN + CHANNEL_ID_LEN;
    // 检测来源名称长度
    let source_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
    if source_len > MAX_NAME_LENGTH as usize {
        return Err(ProtocolError::NameTooLong {
            name_type: "source",
            length: source_len,
            max: MAX_NAME_LENGTH,
        });
    }
    if source_len == 0 {
        return Err(ProtocolError::EmptyName("source"));
    }
    offset += SOURCE_LEN_SIZE;
    // 获取来源名称
    let source_name = std::str::from_utf8(&bytes[offset..offset + source_len])
        .map_err(|_| ProtocolError::InvalidName("source"))?
        .to_string();
    offset += source_len;
    // 检测目标名称长度
    let target_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
    if target_len > MAX_NAME_LENGTH as usize {
        return Err(ProtocolError::NameTooLong {
            name_type: "target",
            length: target_len,
            max: MAX_NAME_LENGTH,
        });
    }
    if target_len == 0 {
        return Err(ProtocolError::EmptyName("target"));
    }
    offset += TARGET_LEN_SIZE;
    // 获取目标名称
    let target_name = std::str::from_utf8(&bytes[offset..offset + target_len])
        .map_err(|_| ProtocolError::InvalidName("target"))?
        .to_string();
    // 输出
    Ok((source_name, target_name))
}

/// 解析消息
pub fn parse_message(bytes: &[u8]) -> ProtocolResult<(MessageHeader, &[u8])> {
    // 检测帧长度，小于 MESSAGE_FIXED_LEN 报错
    if bytes.len() < MESSAGE_FIXED_LEN {
        return Err(ProtocolError::IncompleteFrame {
            required: MESSAGE_FIXED_LEN,
            actual: bytes.len(),
            context: "channel id",
        });
    }

    // 提取通道 ID 字节范围 [FIXED_HEADER_LEN..FIXED_HEADER_LEN+CHANNEL_ID_LEN]
    let channel_id_start = FIXED_HEADER_LEN;
    let channel_id_end = FIXED_HEADER_LEN + CHANNEL_ID_LEN;

    // 转换为 ChannelId
    let chan_id = ChannelId::try_from_slice(&bytes[channel_id_start..channel_id_end])?;

    // 获取来源和目标名字
    // 获取信息来源和目标
    let mut offset = FIXED_HEADER_LEN + CHANNEL_ID_LEN;
    // 检测来源名称长度
    let source_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
    if source_len > MAX_NAME_LENGTH as usize {
        return Err(ProtocolError::NameTooLong {
            name_type: "source",
            length: source_len,
            max: MAX_NAME_LENGTH,
        });
    }
    if source_len == 0 {
        return Err(ProtocolError::EmptyName("source"));
    }
    offset += SOURCE_LEN_SIZE;
    // 获取来源名称
    let source_name = std::str::from_utf8(&bytes[offset..offset + source_len])
        .map_err(|_| ProtocolError::InvalidName("source"))?
        .to_string();
    offset += source_len;
    // 检测目标名称长度
    let target_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
    if target_len > MAX_NAME_LENGTH as usize {
        return Err(ProtocolError::NameTooLong {
            name_type: "target",
            length: target_len,
            max: MAX_NAME_LENGTH,
        });
    }
    if target_len == 0 {
        return Err(ProtocolError::EmptyName("target"));
    }
    offset += TARGET_LEN_SIZE;
    // 获取目标名称
    let target_name = std::str::from_utf8(&bytes[offset..offset + target_len])
        .map_err(|_| ProtocolError::InvalidName("target"))?
        .to_string();
    offset += target_len;
    // 获取流数据总长度
    let total_len = u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]) as usize;
    offset += TOTAL_STREAM_LEN;
    // 获取偏移量
    let stream_offset = u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]) as usize;
    offset += STREAM_OFFSET_LEN;
    // 获取payload
    let payload = &bytes[offset..bytes.len() - 2];
    // 返回结果
    let header = MessageHeader {
        channel_id: chan_id,
        source: source_name,
        target: target_name,
        total_len,
        stream_offset,
    };

    Ok((header, payload))
}
