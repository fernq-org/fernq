use super::encoder::generate_message_data_stream;
use super::error::{ProtocolError, ProtocolResult};
use super::frame::{ChannelId, MessageType};
use super::message::{ContentType, encode_response_to_bytes};
use uuid::Uuid;

/// 验证URL格式，返回可直接连接的地址,和编码后可发送的信息帧
/// 输入参数:
///   - roomURL: 目标URL（如 "fernq://node-a.local:8080/uuid#room?room_pass=secret"）
///
/// 输出:
///   - ("node-a.local:8080", []byte(encoded), nil)  域名+端口
///
/// 注意：
///      - ("node-a.local", []byte(encoded), nil)        域名无端口
///      - ("192.168.1.100:9147", []byte(encoded), nil)  IP无端口时用默认9147
///      - ("[::1]:9147", []byte(encoded), nil)          IPv6无端口时用默认9147
pub fn create_validate(
    name: &str,
    channel_id: &ChannelId,
    url: String,
) -> ProtocolResult<(String, Vec<u8>)> {
    // 1. 检查协议头
    const PREFIX: &str = "fernq://";
    if !url.starts_with(PREFIX) {
        return Err(ProtocolError::InvalidProtocol(url));
    }

    // 2. 去掉协议头，提取地址部分（到路径/参数/锚点为止）
    let rest = &url[PREFIX.len()..];
    let addr_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let addr_part = &rest[..addr_end];

    // 3. 检查地址为空
    if addr_part.is_empty() {
        return Err(ProtocolError::EmptyAddress);
    }

    // 4. 验证地址格式并规范化
    let normalized_addr = validate_and_normalize_address(addr_part)?;

    // 5. 编码
    let mut frames = generate_message_data_stream(
        MessageType::Header,
        true,
        false,
        channel_id,
        name,
        "fernq",
        url.as_bytes(),
    )?;
    // 6. 检查帧生成结果，必须只有一个帧
    if frames.len() != 1 {
        return Err(ProtocolError::InvalidFrameLength("frame too long".into()));
    }
    // 7. 返回
    let frame = frames.remove(0);
    Ok((normalized_addr, frame))
}

/// 解析地址
///
/// 输入：
/// - 验证的信息payload，(url.as_bytes())
///
/// 输出：
/// - UUID
/// - 名称
/// - 密码
pub fn parse_validate(payload: &[u8]) -> ProtocolResult<(Uuid, String, String)> {
    // 1. UTF-8 编码检查
    let url = std::str::from_utf8(payload).map_err(|_| ProtocolError::InvalidUtf8)?;

    // 2. 协议头检查（防止收到畸形数据）
    if !url.starts_with("fernq://") {
        return Err(ProtocolError::InvalidProtocol(url.to_string()));
    }

    // 3. 提取密码（从 query string 中）
    let (url_without_query, password) = extract_password(url)?;

    // 4. 提取房间名称（从 fragment 中）
    let (path_part, name) = extract_name(url_without_query)?;

    // 5. 提取并验证 UUID（路径最后一段）
    let uuid = extract_uuid(path_part)?;

    Ok((uuid, name, password))
}

/// 生成验证信息的响应帧
pub fn create_verify_response(
    name: &str,
    channel_id: &ChannelId,
    state: bool,
    message: &str,
) -> ProtocolResult<Vec<u8>> {
    let json_obj = serde_json::json!({
        "state": state,
        "message": message
    });
    // 编码
    let mut frames = generate_message_data_stream(
        MessageType::Header,
        true,
        true,
        channel_id,
        "fernq",
        name,
        json_obj.to_string().as_bytes(),
    )?;
    // 验证frames长度必须是1
    if frames.len() != 1 {
        return Err(ProtocolError::InvalidFrameLength("frame too long".into()));
    }
    let frame = frames.remove(0);
    Ok(frame)
}

/// 生成响应信息帧
pub fn create_message_response(
    name: &str,
    channel_id: &ChannelId,
    state: u16,
    body: &str,
) -> ProtocolResult<Vec<u8>> {
    let json_obj = encode_response_to_bytes(
        state,
        ContentType::Json,
        Some(serde_json::Value::String(body.to_string())),
    )?;
    // 编码
    let mut frames = generate_message_data_stream(
        MessageType::Header,
        true,
        true,
        channel_id,
        "fernq",
        name,
        &json_obj,
    )?;
    // 验证frames长度必须是1
    if frames.len() != 1 {
        return Err(ProtocolError::InvalidFrameLength("frame too long".into()));
    }
    let frame = frames.remove(0);
    Ok(frame)
}

/// 解析验证响应信息
pub fn parse_verify_response(data: &[u8]) -> ProtocolResult<(bool, String)> {
    let json_str = std::str::from_utf8(data).map_err(|_| ProtocolError::InvalidUtf8)?;

    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| ProtocolError::InvalidJson(e.to_string()))?;

    let state = value["state"]
        .as_bool()
        .ok_or_else(|| ProtocolError::InvalidJson("field 'state' must be boolean".into()))?;

    let message = value["message"]
        .as_str()
        .ok_or_else(|| ProtocolError::InvalidJson("field 'message' must be string".into()))?
        .to_string();

    Ok((state, message))
}

/// 验证地址格式并返回规范化地址
fn validate_and_normalize_address(addr: &str) -> ProtocolResult<String> {
    // 检查IPv6格式 [addr]:port 或 [addr]
    if addr.starts_with('[') {
        validate_ipv6_address(addr)
    } else {
        // IPv4或域名格式
        validate_ipv4_or_hostname(addr)
    }
}

/// 验证IPv4格式
fn validate_ipv4_or_hostname(addr: &str) -> ProtocolResult<String> {
    match addr.rfind(':') {
        Some(colon_pos) => {
            // 可能有端口，需要区分IPv4和域名
            let (host, port_str) = addr.split_at(colon_pos);
            let port_str = &port_str[1..]; // 去掉:

            // 验证端口是数字且在有效范围
            match port_str.parse::<u16>() {
                Ok(_) => {
                    // 检查主机部分不为空
                    if host.is_empty() {
                        return Err(ProtocolError::MalformedFrame(
                            "empty host before port".to_string(),
                        ));
                    }
                    Ok(addr.to_string())
                }
                Err(_) => Err(ProtocolError::InvalidPort(port_str.to_string())),
            }
        }
        None => {
            // 无端口，返回原地址（使用默认端口的逻辑交给调用方）
            Ok(addr.to_string())
        }
    }
}

/// 验证 IPv6格式
fn validate_ipv6_address(addr: &str) -> ProtocolResult<String> {
    // 查找结束括号
    let close_bracket = addr
        .find(']')
        .ok_or_else(|| ProtocolError::MalformedFrame("unclosed IPv6 bracket".to_string()))?;

    // 检查括号后是否有端口 :port
    if close_bracket + 1 < addr.len() {
        if &addr[close_bracket + 1..close_bracket + 2] != ":" {
            return Err(ProtocolError::MalformedFrame(
                "invalid IPv6 format, expected ]:port".to_string(),
            ));
        }
        let port_str = &addr[close_bracket + 2..];
        if port_str.parse::<u16>().is_err() {
            return Err(ProtocolError::InvalidPort(port_str.to_string()));
        }
    }

    Ok(addr.to_string())
}

/// 从 URL 中提取 room_pass 参数
fn extract_password(url: &str) -> ProtocolResult<(&str, String)> {
    let query_start = url.find('?').ok_or(ProtocolError::MissingPassword)?;

    let base_url = &url[..query_start];
    let query = &url[query_start + 1..];

    // 在查询参数中查找 room_pass=
    let password = query
        .split('&')
        .find_map(|param| param.strip_prefix("room_pass="))
        .ok_or(ProtocolError::MissingPassword)?;

    if password.is_empty() {
        return Err(ProtocolError::EmptyPassword);
    }

    Ok((base_url, password.to_string()))
}

/// 从 URL 中提取 fragment（#后面的名称）
fn extract_name(url: &str) -> ProtocolResult<(&str, String)> {
    let hash_pos = url.find('#').ok_or(ProtocolError::MissingName)?;

    let base = &url[..hash_pos];
    let name = &url[hash_pos + 1..];

    // 检查名称是否为空
    if name.is_empty() {
        return Err(ProtocolError::MissingName);
    }

    Ok((base, name.to_string()))
}

/// 从路径中提取并验证 UUID
fn extract_uuid(url: &str) -> ProtocolResult<Uuid> {
    // 取最后一个 / 后面的内容作为 UUID
    let uuid_str = url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .ok_or(ProtocolError::MissingUuid)?;

    // 验证 UUID 格式（标准 UUID 格式：550e8400-e29b-41d4-a716-446655440000）
    Uuid::parse_str(uuid_str).map_err(|e| {
        ProtocolError::InvalidUuidFormat(format!("'{}' is not a valid UUID: {}", uuid_str, e))
    })
}
