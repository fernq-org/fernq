use std::vec;

use super::error::{ProtocolError, ProtocolResult};
use super::frame::{ChannelId, MessageFlags, MessageType};
use super::{
    FIXED_HEADER_LEN, MAGIC, MAGIC_LEN, MAX_FRAME_SIZE, MAX_NAME_LENGTH, MAX_STREAM_LENGTH,
    MESSAGE_FIXED_LEN, NET_CRC, VERSION,
};

/// 生成Ping帧
pub fn ping_frame() -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&MAGIC.to_be_bytes()); // 4 字节
    frame.push(VERSION); // 1 字节
    frame.push(MessageType::Ping.as_u8()); // 1 字节
    frame.push(MessageFlags::new(true, true, false).as_u8()); // 1 字节
    frame.extend_from_slice(&2_u32.to_be_bytes()); // 4 字节, 补充非固定部分后面的长度为 2 字节
    // 使用NET_CRC生成CRC16校验和
    frame.extend_from_slice(&NET_CRC.checksum(&frame[MAGIC_LEN..]).to_be_bytes());
    frame
}

/// 生成Pong帧
pub fn pong_frame() -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&MAGIC.to_be_bytes()); // 4 字节
    frame.push(VERSION); // 1 字节
    frame.push(MessageType::Pong.as_u8()); // 1 字节
    frame.push(MessageFlags::new(true, true, true).as_u8()); // 1 字节
    frame.extend_from_slice(&2_u32.to_be_bytes()); // 4 字节, 补充非固定部分后面的长度为 2 字节
    // 使用NET_CRC生成CRC16校验和
    frame.extend_from_slice(&NET_CRC.checksum(&frame[MAGIC_LEN..]).to_be_bytes());
    frame
}

/// 生成消息数据流
///
/// 协议约束（Protocol Constraints）
/// - 单帧最大 8KB (MAX_FRAME_SIZE)
/// - 流最大 8MB (MAX_STREAM_LENGTH)
/// - 名字最大 128 字节 (MAX_NAME_LENGTH)
///
/// 修改历史
/// - v1.0: 基础实现，支持分帧
pub fn generate_message_data_stream(
    message_type: MessageType,
    end_channel: bool,
    is_response: bool,
    channel_id: &ChannelId,
    source_name: &str,
    target_name: &str,
    data: &[u8],
) -> ProtocolResult<Vec<Vec<u8>>> {
    // 声明存储变量
    let mut frames = vec![];
    // 验证名字字符串长度转换成bytes,并获取长度
    if source_name.is_empty() {
        return Err(ProtocolError::EmptyName("source"));
    }
    if target_name.is_empty() {
        return Err(ProtocolError::EmptyName("target"));
    }
    if source_name.len() > MAX_NAME_LENGTH as usize {
        return Err(ProtocolError::NameTooLong {
            name_type: "source",
            length: source_name.len(),
            max: MAX_NAME_LENGTH,
        });
    }
    if target_name.len() > MAX_NAME_LENGTH as usize {
        return Err(ProtocolError::NameTooLong {
            name_type: "target",
            length: target_name.len(),
            max: MAX_NAME_LENGTH,
        });
    }
    let source_name_bytes = source_name.as_bytes();
    let target_name_bytes = target_name.as_bytes();
    let source_name_len = source_name_bytes.len();
    let target_name_len = target_name_bytes.len();

    // 计算每一个帧payload的最大长度
    // 安全：MAX_FRAME_SIZE(8KB) >> MAX_NAME_LENGTH(128)*2 + MESSAGE_FIXED_LEN
    let max_payload_len = MAX_FRAME_SIZE - MESSAGE_FIXED_LEN - source_name_len - target_name_len;
    if max_payload_len == 0 {
        return Err(ProtocolError::InvalidFrameLength(
            "names too long, no space for payload".into(),
        ));
    }
    // 声明总数据流长度和数据帧偏移量
    let total_stream_length = data.len();
    if total_stream_length > MAX_STREAM_LENGTH as usize {
        return Err(ProtocolError::StreamTooLong {
            length: total_stream_length,
            max: MAX_STREAM_LENGTH,
        });
    }
    let mut offset = 0;
    // 循环获取数据帧
    while offset < total_stream_length {
        let payload_len = std::cmp::min(max_payload_len, total_stream_length - offset);
        // 预分配精确容量，零重新分配
        let capacity = MESSAGE_FIXED_LEN + source_name_len + target_name_len + payload_len;
        let frame_len = capacity - FIXED_HEADER_LEN;
        let mut frame = Vec::with_capacity(capacity);
        // 填充魔数
        frame.extend_from_slice(&MAGIC.to_be_bytes());
        // 填充版本
        frame.push(VERSION);
        // 填充消息类型
        frame.push(message_type.as_u8());
        // 判断是否结束, 填充标志位
        frame.push(
            MessageFlags::new(
                offset + payload_len >= total_stream_length,
                end_channel,
                is_response,
            )
            .as_u8(),
        );
        // 填充帧长度
        frame.extend_from_slice(&(frame_len as u32).to_be_bytes());
        // 填充 Channel ID
        frame.extend_from_slice(channel_id.as_bytes());
        // 填充 Source name 长度
        frame.extend_from_slice(&(source_name_len as u16).to_be_bytes());
        // 填充 Source name
        frame.extend_from_slice(source_name_bytes);
        // 填充 Target name 长度
        frame.extend_from_slice(&(target_name_len as u16).to_be_bytes());
        // 填充 Target name
        frame.extend_from_slice(target_name_bytes);
        // 填充 Stream 总长度
        frame.extend_from_slice(&(total_stream_length as u32).to_be_bytes());
        // 填充 Stream 偏移
        frame.extend_from_slice(&(offset as u32).to_be_bytes());
        // 填充payload数据
        frame.extend_from_slice(&data[offset..offset + payload_len]);
        // 填充 CRC
        frame.extend_from_slice(&NET_CRC.checksum(&frame[MAGIC_LEN..]).to_be_bytes());
        // 写入帧数组
        frames.push(frame);
        // 移动偏移
        offset += payload_len;
    }
    Ok(frames)
}
