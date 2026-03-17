use super::error::{ProtocolError, ProtocolResult};
use bytes::Bytes;
use serde_json::{Map, Value};

/// Header 类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderType {
    Request,
    Response,
}

impl HeaderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            HeaderType::Request => "request",
            HeaderType::Response => "response",
        }
    }
}

impl std::fmt::Display for HeaderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for HeaderType {
    type Err = ProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "request" => Ok(HeaderType::Request),
            "response" => Ok(HeaderType::Response),
            _ => Err(ProtocolError::InvalidHeaderType(s.to_string())),
        }
    }
}

/// Content 类型枚举（简化版）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    /// JSON 数据，body 可选（可以是任意 Json 值，包括字符串）
    Json,
    /// 表单数据，无 body（流式传输，保留给未来实现）
    Form,
    /// 表单流数据，无 body（流式传输，保留给未来实现）
    FormFlow,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::Json => "json",
            ContentType::Form => "form",
            ContentType::FormFlow => "form_flow",
        }
    }

    /// 是否允许 body 字段（只有 Json 允许，Form/FormFlow 禁止）
    pub fn allows_body(&self) -> bool {
        matches!(self, ContentType::Json)
    }
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ContentType {
    type Err = ProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "json" => Ok(ContentType::Json),
            "form" => Ok(ContentType::Form),
            "form_flow" => Ok(ContentType::FormFlow),
            _ => Err(ProtocolError::InvalidContentType(s.to_string())),
        }
    }
}

/// 解码后的消息结构
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Request(RequestMessage),
    Response(ResponseMessage),
}

/// 请求消息
#[derive(Debug, Clone, PartialEq)]
pub struct RequestMessage {
    pub content_type: ContentType,
    pub path: String,
    pub body: Option<Value>,
}

/// 响应消息
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseMessage {
    pub content_type: ContentType,
    pub state: u16,
    pub body: Option<Value>,
}

impl RequestMessage {
    /// 创建请求（无 body）
    pub fn new(path: impl Into<String>, content_type: ContentType) -> Self {
        Self {
            content_type,
            path: path.into(),
            body: None,
        }
    }

    /// 创建带 body 的请求（自动设置 content_type 为 Json）
    /// body 可以是任意 serde_json::Value，包括字符串: json!("hello")
    pub fn with_body(path: impl Into<String>, body: Value) -> Self {
        Self {
            content_type: ContentType::Json,
            path: path.into(),
            body: Some(body),
        }
    }
}

impl ResponseMessage {
    /// 创建响应（无 body）
    pub fn new(state: u16, content_type: ContentType) -> Self {
        Self {
            content_type,
            state,
            body: None,
        }
    }

    /// 创建带 body 的响应（自动设置 content_type 为 Json）
    pub fn with_body(state: u16, body: Value) -> Self {
        Self {
            content_type: ContentType::Json,
            state,
            body: Some(body),
        }
    }
}

/// 编码请求消息
pub fn encode_request(
    path: impl Into<String>,
    content_type: ContentType,
    body: Option<Value>,
) -> ProtocolResult<Value> {
    // 唯一约束：form/form_flow 绝对不能有 body
    if !content_type.allows_body() && body.is_some() {
        return Err(ProtocolError::UnexpectedBody);
    }

    let mut map = Map::new();
    map.insert(
        "header_type".to_string(),
        Value::String(HeaderType::Request.as_str().to_string()),
    );
    map.insert(
        "content_type".to_string(),
        Value::String(content_type.as_str().to_string()),
    );
    map.insert("path".to_string(), Value::String(path.into()));

    if let Some(b) = body {
        map.insert("body".to_string(), b);
    }

    Ok(Value::Object(map))
}

/// 编码响应消息
pub fn encode_response(
    state: u16,
    content_type: ContentType,
    body: Option<Value>,
) -> ProtocolResult<Value> {
    // 验证 state 范围（HTTP 状态码）
    if !(100..=599).contains(&state) {
        return Err(ProtocolError::InvalidStateCode(state as i64));
    }

    // 唯一约束：form/form_flow 绝对不能有 body
    if !content_type.allows_body() && body.is_some() {
        return Err(ProtocolError::UnexpectedBody);
    }

    let mut map = Map::new();
    map.insert(
        "header_type".to_string(),
        Value::String(HeaderType::Response.as_str().to_string()),
    );
    map.insert(
        "content_type".to_string(),
        Value::String(content_type.as_str().to_string()),
    );
    map.insert(
        "state".to_string(),
        Value::Number(serde_json::Number::from(state)),
    );

    if let Some(b) = body {
        map.insert("body".to_string(), b);
    }

    Ok(Value::Object(map))
}

/// 编码请求为 Bytes
pub fn encode_request_to_bytes(
    path: impl Into<String>,
    content_type: ContentType,
    body: Option<Value>,
) -> ProtocolResult<Bytes> {
    let val = encode_request(path, content_type, body)?;
    let vec = serde_json::to_vec(&val).map_err(|e| ProtocolError::InvalidJson(e.to_string()))?;
    Ok(Bytes::from(vec))
}

/// 编码响应为 Bytes
pub fn encode_response_to_bytes(
    state: u16,
    content_type: ContentType,
    body: Option<Value>,
) -> ProtocolResult<Bytes> {
    let val = encode_response(state, content_type, body)?;
    let vec = serde_json::to_vec(&val).map_err(|e| ProtocolError::InvalidJson(e.to_string()))?;
    Ok(Bytes::from(vec))
}

/// 统一解码
pub fn decode_message(value: Value) -> ProtocolResult<Message> {
    let obj = value
        .as_object()
        .ok_or_else(|| ProtocolError::MalformedFrame("root must be a JSON object".to_string()))?;

    let header_type_str = obj
        .get("header_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ProtocolError::MalformedFrame("missing or invalid header_type".to_string())
        })?;

    let header_type: HeaderType = header_type_str.parse()?;

    let content_type_str = obj
        .get("content_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ProtocolError::MalformedFrame("missing or invalid content_type".to_string())
        })?;

    let content_type: ContentType = content_type_str.parse()?;

    match header_type {
        HeaderType::Request => decode_request(obj, content_type),
        HeaderType::Response => decode_response(obj, content_type),
    }
}

/// 从 &[u8] 解码
pub fn decode_from_bytes(data: &[u8]) -> ProtocolResult<Message> {
    let val: Value =
        serde_json::from_slice(data).map_err(|e| ProtocolError::InvalidJson(e.to_string()))?;
    decode_message(val)
}

/// 内部：解码请求
fn decode_request(obj: &Map<String, Value>, content_type: ContentType) -> ProtocolResult<Message> {
    let path = obj
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or(ProtocolError::MissingPath)?
        .to_string();

    let body = obj.get("body").cloned();

    // 唯一约束检查
    if !content_type.allows_body() && body.is_some() {
        return Err(ProtocolError::UnexpectedBody);
    }

    Ok(Message::Request(RequestMessage {
        content_type,
        path,
        body,
    }))
}

/// 内部：解码响应
fn decode_response(obj: &Map<String, Value>, content_type: ContentType) -> ProtocolResult<Message> {
    let state = obj
        .get("state")
        .and_then(|v| v.as_u64())
        .ok_or(ProtocolError::MissingState)? as u16;

    if !(100..=599).contains(&state) {
        return Err(ProtocolError::InvalidStateCode(state as i64));
    }

    let body = obj.get("body").cloned();

    if !content_type.allows_body() && body.is_some() {
        return Err(ProtocolError::UnexpectedBody);
    }

    Ok(Message::Response(ResponseMessage {
        content_type,
        state,
        body,
    }))
}

/// Value 转 JSON 字符串
pub fn to_json_string(value: &Value) -> ProtocolResult<String> {
    serde_json::to_string(value).map_err(|e| ProtocolError::InvalidJson(e.to_string()))
}

/// JSON 字符串转 Value
pub fn from_json_string(s: &str) -> ProtocolResult<Value> {
    serde_json::from_str(s).map_err(|e| ProtocolError::InvalidJson(e.to_string()))
}

/// 编码请求为字符串
pub fn encode_request_to_string(
    path: impl Into<String>,
    content_type: ContentType,
    body: Option<Value>,
) -> ProtocolResult<String> {
    let val = encode_request(path, content_type, body)?;
    to_json_string(&val)
}

/// 编码响应为字符串
pub fn encode_response_to_string(
    state: u16,
    content_type: ContentType,
    body: Option<Value>,
) -> ProtocolResult<String> {
    let val = encode_response(state, content_type, body)?;
    to_json_string(&val)
}

/// 从字符串解码
pub fn decode_from_string(s: &str) -> ProtocolResult<Message> {
    let val = from_json_string(s)?;
    decode_message(val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_without_body() {
        // Json 可以没有 body
        let bytes = encode_request_to_bytes("/health", ContentType::Json, None).unwrap();
        let msg = decode_from_bytes(&bytes).unwrap();

        match msg {
            Message::Request(req) => {
                assert_eq!(req.content_type, ContentType::Json);
                assert!(req.body.is_none());
            }
            _ => panic!("expected request"),
        }
    }

    #[test]
    fn test_json_with_string_body() {
        // Json 可以包含字符串（通过 json! 宏）
        let body = json!("hello world");
        let bytes = encode_request_to_bytes("/echo", ContentType::Json, Some(body)).unwrap();
        let msg = decode_from_bytes(&bytes).unwrap();

        match msg {
            Message::Request(req) => {
                assert_eq!(req.body, Some(json!("hello world")));
            }
            _ => panic!("expected request"),
        }
    }

    #[test]
    fn test_form_must_not_have_body() {
        // Form 绝对不能有 body
        let result =
            encode_request_to_bytes("/upload", ContentType::Form, Some(json!({"file": "test"})));
        assert!(matches!(result, Err(ProtocolError::UnexpectedBody)));
    }

    #[test]
    fn test_response_json_optional_body() {
        // 响应也可以没有 body（如 204 No Content）
        let bytes = encode_response_to_bytes(204, ContentType::Json, None).unwrap();
        let msg = decode_from_bytes(&bytes).unwrap();

        match msg {
            Message::Response(res) => {
                assert_eq!(res.state, 204);
                assert!(res.body.is_none());
            }
            _ => panic!("expected response"),
        }
    }
}
