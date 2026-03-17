// #![allow(dead_code)]
use crc::{CRC_16_XMODEM, Crc};
// 定义算法常量（全局）
const NET_CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_XMODEM);

// ========== 核心常量（所有子模块共享）==========
const MAGIC: u32 = 20262351;
const VERSION: u8 = 1;

// 固定字节长度（便于计算偏移）
const MAGIC_LEN: usize = 4; // 魔数 长度 4字节
const VERSION_LEN: usize = 1; // 版本 长度 1字节
const TYPE_LEN: usize = 1; // 类型 长度 1字节
const FLAGS_LEN: usize = 1; // 标志 长度 1字节
const FRAME_LENGTH_LEN: usize = 4; // 当前帧的长度，具体指channelid到CRC 长度 4字节
const CHANNEL_ID_LEN: usize = 16; // Channel ID 长度 16字节
const SOURCE_LEN_SIZE: usize = 2; // Source name 标识 长度 2字节
const TARGET_LEN_SIZE: usize = 2; // Target name 标识 长度 2字节
const TOTAL_STREAM_LEN: usize = 4; // Stream 总长度 长度 4字节
const STREAM_OFFSET_LEN: usize = 4; // Stream 偏移 长度 4字节
const CRC_LEN: usize = 2; // CRC 长度 2字节

// 计算得到的固定头部长度（到 frame length 结束）
pub const FIXED_HEADER_LEN: usize =
    MAGIC_LEN + VERSION_LEN + TYPE_LEN + FLAGS_LEN + FRAME_LENGTH_LEN; // = 11 
pub const MESSAGE_FIXED_LEN: usize = FIXED_HEADER_LEN
    + CHANNEL_ID_LEN
    + SOURCE_LEN_SIZE
    + TARGET_LEN_SIZE
    + TOTAL_STREAM_LEN
    + STREAM_OFFSET_LEN
    + CRC_LEN; // = 41

// 限制常量
pub const MAX_FRAME_SIZE: usize = 8 * 1024; // 8KB
pub const MAX_STREAM_LENGTH: u32 = 8 * 1024 * 1024; // 8MB
pub const MAX_NAME_LENGTH: u16 = 128; // Source/Target 最大长度

// ========== 子模块声明 ==========
mod decoder;
mod encoder;
mod error;
mod frame;
mod message;
mod validate;

// 默认导出
pub use decoder::{
    decode_basic, decode_header, get_channel_id, get_message_source_target, parse_message,
};
pub use encoder::{generate_message_data_stream, ping_frame, pong_frame};
pub use error::{ProtocolError, ProtocolResult};
pub use frame::{ChannelId, MessageFlags, MessageHeader, MessageType};
pub use message::{
    Message, RequestMessage, ResponseMessage, decode_from_bytes, decode_from_string,
    decode_message, encode_request, encode_request_to_bytes, encode_request_to_string,
    encode_response, encode_response_to_bytes, encode_response_to_string,
};
pub use validate::{
    create_message_response, create_validate, create_verify_response, parse_validate,
    parse_verify_response,
};
