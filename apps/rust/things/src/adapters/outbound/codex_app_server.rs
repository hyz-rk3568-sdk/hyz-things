//! Fixed-LAN Codex App Server adapter for the single-user study site.
//! Codex signs in once on the PC; this device only holds a capability token.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub const SERVER: &str = "192.168.8.202:1457";
const MAX_PACKET: usize = 256 * 1024;
const MAX_EVENTS: usize = 2048;
pub const TURN_TIMEOUT: Duration = Duration::from_secs(85);

/// A fixed endpoint: never allow browser input or AI-generated content to select a host.
pub struct CodexSession {
    socket: TcpStream,
    thread_id: String,
    request_id: i64,
}

fn accept_key(key: &str) -> String {
    let mut sha = Sha1::new();
    sha.update(key.as_bytes());
    sha.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    STANDARD.encode(sha.finalize())
}

pub(super) fn validate_token(token: &str) -> Result<(), String> {
    let bytes = token.as_bytes();
    let padding_start = bytes
        .iter()
        .position(|byte| *byte == b'=')
        .unwrap_or(bytes.len());
    let valid =
        padding_start > 0 && bytes[..padding_start].iter().all(|byte| {
            matches!(
                *byte,
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'+' | b'/'
            )
        }) && bytes[padding_start..].iter().all(|byte| *byte == b'=');
    if !valid {
        return Err("Codex Token must be a non-empty bearer token".into());
    }
    Ok(())
}

fn output_schema() -> Value {
    let properties = json!({
        "question":{"type":"string"},
        "evidence":{"type":"string"},
        "operation":{"type":"string","enum":[
            "base_period","growth_amount","growth_rate","share","average","difference","ratio"
        ]},
        "a":{"type":"number"},
        "b":{"type":"number"},
        "unit":{"type":"string"},
        "explanation":{"type":"string"}
    });
    json!({
        "type":"object",
        "properties":{
            "material_text":{"type":"string"},
            "questions":{"type":"array","items":{
                "type":"object",
                "properties":properties,
                "required":["question","evidence","operation","a","b","unit","explanation"],
                "additionalProperties":false
            }}
        },
        "required":["material_text","questions"],
        "additionalProperties":false
    })
}

async fn handshake(socket: &mut TcpStream, host: &str, token: &str) -> Result<(), String> {
    validate_token(token)?;
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Codex 握手随机源不可用")?;
    let key = STANDARD.encode(bytes);
    let request = format!(
        "GET / HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {key}\r\nAuthorization: Bearer {token}\r\n\r\n"
    );
    socket
        .write_all(request.as_bytes())
        .await
        .map_err(|_| "无法发送 Codex 握手")?;
    // Read exactly through CRLFCRLF so we don't consume the first WebSocket frame.
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= 8192 {
            return Err("Codex 握手响应过大".into());
        }
        let mut b = [0u8];
        socket
            .read_exact(&mut b)
            .await
            .map_err(|_| "无法读取 Codex 握手")?;
        header.push(b[0]);
    }
    let response = std::str::from_utf8(&header).map_err(|_| "Codex 握手头格式无效")?;
    if response.starts_with("HTTP/1.1 401") || response.starts_with("HTTP/1.1 403") {
        return Err("Codex WebSocket Token 被拒绝，请检查电脑与 RK3568 的 Token 是否一致".into());
    }
    if !response.starts_with("HTTP/1.1 101 ") {
        return Err("Codex App Server 握手失败；确认电脑已启动 WebSocket 服务".into());
    }
    let mut valid_accept = false;
    let mut valid_upgrade = false;
    let mut valid_connection = false;
    for line in response.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            if name.eq_ignore_ascii_case("sec-websocket-accept") {
                valid_accept = value == accept_key(&key);
            } else if name.eq_ignore_ascii_case("upgrade") {
                valid_upgrade = value.eq_ignore_ascii_case("websocket");
            } else if name.eq_ignore_ascii_case("connection") {
                valid_connection = value
                    .split(',')
                    .any(|v| v.trim().eq_ignore_ascii_case("upgrade"));
            }
        }
    }
    if !(valid_accept && valid_upgrade && valid_connection) {
        return Err("Codex WebSocket 握手签名不匹配".into());
    }
    Ok(())
}

async fn write_frame(socket: &mut TcpStream, opcode: u8, payload: &[u8]) -> Result<(), String> {
    if payload.len() > MAX_PACKET {
        return Err("Codex 请求超过大小限制".into());
    }
    let mut mask = [0u8; 4];
    getrandom::fill(&mut mask).map_err(|_| "Codex 帧掩码随机源不可用")?;
    let mut frame = Vec::with_capacity(payload.len() + 16);
    frame.push(0x80 | opcode);
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(i, byte)| byte ^ mask[i % 4]),
    );
    socket
        .write_all(&frame)
        .await
        .map_err(|_| "Codex WebSocket 写入失败".into())
}

async fn read_text(socket: &mut TcpStream) -> Result<String, String> {
    let mut message = Vec::new();
    let mut fragmented = false;
    loop {
        let mut head = [0u8; 2];
        socket
            .read_exact(&mut head)
            .await
            .map_err(|_| "Codex WebSocket 连接已中断")?;
        let final_frame = head[0] & 0x80 != 0;
        let opcode = head[0] & 0x0f;
        if head[0] & 0x70 != 0 || head[1] & 0x80 != 0 {
            return Err("Codex WebSocket 返回不支持的帧格式".into());
        }
        let mut length = (head[1] & 0x7f) as u64;
        if length == 126 {
            let mut bytes = [0u8; 2];
            socket
                .read_exact(&mut bytes)
                .await
                .map_err(|_| "Codex 帧长度截断")?;
            length = u16::from_be_bytes(bytes) as u64;
        } else if length == 127 {
            let mut bytes = [0u8; 8];
            socket
                .read_exact(&mut bytes)
                .await
                .map_err(|_| "Codex 帧长度截断")?;
            length = u64::from_be_bytes(bytes);
        }
        if length > MAX_PACKET as u64 || message.len().saturating_add(length as usize) > MAX_PACKET
        {
            return Err("Codex 响应超过大小限制".into());
        }
        if opcode >= 8 && (!final_frame || length > 125) {
            return Err("Codex WebSocket 控制帧无效".into());
        }
        let mut payload = vec![0u8; length as usize];
        socket
            .read_exact(&mut payload)
            .await
            .map_err(|_| "Codex 帧读取失败")?;
        match opcode {
            0x8 => return Err("Codex App Server 已关闭连接".into()),
            0x9 => write_frame(socket, 0xA, &payload).await?,
            0xA => {}
            0x1 if !fragmented => {
                message.extend_from_slice(&payload);
                fragmented = !final_frame;
                if final_frame {
                    return String::from_utf8(message).map_err(|_| "Codex 响应不是 UTF-8".into());
                }
            }
            0x0 if fragmented => {
                message.extend_from_slice(&payload);
                if final_frame {
                    return String::from_utf8(message).map_err(|_| "Codex 响应不是 UTF-8".into());
                }
            }
            _ => return Err("Codex WebSocket 收到不支持的数据帧".into()),
        }
    }
}

impl CodexSession {
    pub async fn connect(token: &str) -> Result<Self, String> {
        Self::connect_at(SERVER, token).await
    }

    async fn connect_at(address: &str, token: &str) -> Result<Self, String> {
        validate_token(token)?;
        let mut socket = TcpStream::connect(address)
            .await
            .map_err(|_| "无法连接 192.168.8.202:1457；请检查电脑 Codex App Server")?;
        handshake(&mut socket, address, token).await?;
        let mut client = Self {
            socket,
            thread_id: String::new(),
            request_id: 0,
        };
        client
            .request(
                "initialize",
                json!({
                    "clientInfo":{"name":"hyz_things","title":"hyz-things Study","version":"0.1.0"}
                }),
            )
            .await?;
        client.notify("initialized", json!({})).await?;
        let thread = client
            .request(
                "thread/start",
                json!({
                    "ephemeral":true,
                    "cwd":"/tmp",
                    "approvalPolicy":"never",
                    "sandbox":"read-only"
                }),
            )
            .await?;
        client.thread_id = thread
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or("Codex 未返回有效 Thread ID")?
            .to_owned();
        Ok(client)
    }

    async fn send(&mut self, message: &Value) -> Result<(), String> {
        let bytes = serde_json::to_vec(message).map_err(|_| "Codex JSON-RPC 编码失败")?;
        write_frame(&mut self.socket, 1, &bytes).await
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({"method":method,"params":params})).await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.request_id += 1;
        let id = self.request_id;
        self.send(&json!({"id":id,"method":method,"params":params}))
            .await?;
        for _ in 0..MAX_EVENTS {
            let text = read_text(&mut self.socket).await?;
            let msg: Value = serde_json::from_str(&text).map_err(|_| "Codex 返回非法 JSON-RPC")?;
            if msg.get("id").and_then(Value::as_i64) != Some(id) {
                continue;
            }
            if msg.get("error").is_some() {
                return Err("Codex 拒绝 JSON-RPC 请求；请检查版本和登录状态".into());
            }
            return msg
                .get("result")
                .cloned()
                .ok_or("Codex 响应缺少 result".into());
        }
        Err("Codex JSON-RPC 初始化事件过多".into())
    }

    /// Run one generation or repair turn in the same ephemeral thread.
    pub async fn turn(&mut self, prompt: &str) -> Result<String, String> {
        let thread = self.thread_id.clone();
        let result = self
            .request(
                "turn/start",
                json!({
                    "threadId":thread,
                    "input":[{"type":"text","text":prompt}],
                    "outputSchema":output_schema()
                }),
            )
            .await?;
        let turn_id = result
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .ok_or("Codex 未返回有效 Turn ID")?
            .to_owned();
        let mut final_answer = String::new();
        for _ in 0..MAX_EVENTS {
            let raw = read_text(&mut self.socket).await?;
            let packet: Value =
                serde_json::from_str(&raw).map_err(|_| "Codex 返回非法 JSON-RPC 通知")?;
            // Never authorize commands, file edits or interactive prompts.
            if let Some(id) = packet.get("id") {
                self.send(&json!({"id":id,"error":{"code":-32601,"message":"Interactive tools unavailable"}})).await?;
                continue;
            }
            if packet.pointer("/params/threadId").and_then(Value::as_str) != Some(thread.as_str()) {
                continue;
            }
            let method = packet.get("method").and_then(Value::as_str);
            if packet.pointer("/params/turnId").and_then(Value::as_str) == Some(turn_id.as_str())
                && method == Some("item/completed")
                && packet.pointer("/params/item/type").and_then(Value::as_str)
                    == Some("agentMessage")
            {
                if let Some(content) = packet.pointer("/params/item/text").and_then(Value::as_str) {
                    final_answer = content.to_owned();
                }
            }
            if method == Some("turn/completed")
                && packet.pointer("/params/turn/id").and_then(Value::as_str) == Some(&turn_id)
            {
                if packet
                    .pointer("/params/turn/status")
                    .and_then(Value::as_str)
                    != Some("completed")
                {
                    return Err("Codex 生成失败或中断（可能登录失效或额度不足）".into());
                }
                if final_answer.is_empty() || final_answer.len() > MAX_PACKET {
                    return Err("Codex 没有生成完整的题目 JSON".into());
                }
                return Ok(final_answer);
            }
        }
        Err("Codex 生成过程事件过多".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn bearer_token_accepts_opaque_values_but_rejects_unsafe_headers() {
        assert!(validate_token("").is_err());
        assert!(validate_token("bad token").is_err());
        assert!(validate_token(&"x".repeat(64)).is_ok());
        assert!(validate_token(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn output_schema_has_required_recipe_fields() {
        let schema = output_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["properties"]["questions"]["items"]["additionalProperties"],
            false
        );
        assert_eq!(
            schema["properties"]["questions"]["items"]["required"]
                .as_array()
                .unwrap()
                .len(),
            7
        );
    }

    #[test]
    fn bearer_validation_accepts_non_hex_token_without_a_fixed_length() {
        assert!(validate_token("short-token-12").is_ok());
    }

    #[test]
    fn bearer_validation_rejects_empty_or_header_unsafe_values() {
        assert!(validate_token("").is_err());
        assert!(validate_token("safe token").is_err());
        assert!(validate_token("safe\r\nX-Injected: true").is_err());
    }

    #[test]
    fn handshake_accept_matches_known_websocket_vector() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    async fn server_message(socket: &mut TcpStream, msg: Value) {
        let bytes = serde_json::to_vec(&msg).unwrap();
        if bytes.len() < 126 {
            socket.write_all(&[0x81, bytes.len() as u8]).await.unwrap();
        } else {
            socket.write_all(&[0x81, 126]).await.unwrap();
            socket
                .write_all(&(bytes.len() as u16).to_be_bytes())
                .await
                .unwrap();
        }
        socket.write_all(&bytes).await.unwrap();
    }

    async fn client_message(socket: &mut TcpStream) -> Value {
        let mut header = [0u8; 2];
        socket.read_exact(&mut header).await.unwrap();
        assert_eq!(header[0], 0x81);
        assert_eq!(header[1] & 0x80, 0x80);
        let len = (header[1] & 0x7f) as usize;
        let length = if len == 126 {
            let mut next = [0u8; 2];
            socket.read_exact(&mut next).await.unwrap();
            u16::from_be_bytes(next) as usize
        } else {
            len
        };
        let mut mask = [0u8; 4];
        socket.read_exact(&mut mask).await.unwrap();
        let mut body = vec![0u8; length];
        socket.read_exact(&mut body).await.unwrap();
        for (idx, byte) in body.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn fake_codex_app_server_completes_generation_without_external_network() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mock = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut handshake = Vec::new();
            while !handshake.ends_with(b"\r\n\r\n") {
                let mut b = [0u8];
                socket.read_exact(&mut b).await.unwrap();
                handshake.push(b[0]);
            }
            let head = String::from_utf8(handshake).unwrap();
            assert!(head.contains(&format!("Authorization: Bearer {}", "a".repeat(64))));
            let key = head
                .lines()
                .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
                .unwrap()
                .trim();
            let reply = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",accept_key(key));
            socket.write_all(reply.as_bytes()).await.unwrap();
            let init = client_message(&mut socket).await;
            assert_eq!(init["method"], "initialize");
            server_message(&mut socket, json!({"id":1,"result":{}})).await;
            let initialized = client_message(&mut socket).await;
            assert_eq!(initialized["method"], "initialized");
            let thread = client_message(&mut socket).await;
            assert_eq!(thread["method"], "thread/start");
            assert_eq!(thread["params"]["ephemeral"], true);
            server_message(
                &mut socket,
                json!({"id":2,"result":{"thread":{"id":"test-thread"}}}),
            )
            .await;
            let turn = client_message(&mut socket).await;
            assert_eq!(turn["method"], "turn/start");
            assert_eq!(turn["params"]["outputSchema"]["type"], "object");
            server_message(
                &mut socket,
                json!({"id":3,"result":{"turn":{"id":"turn-1"}}}),
            )
            .await;
            server_message(&mut socket,json!({"method":"item/completed","params":{
                "threadId":"test-thread","turnId":"turn-1",
                "item":{"type":"agentMessage","text":"{\"material_text\":\"测试\",\"questions\":[]}"}
            }})).await;
            server_message(
                &mut socket,
                json!({"method":"turn/completed","params":{
                    "threadId":"test-thread","turn":{"id":"turn-1","status":"completed"}
                }}),
            )
            .await;
        });
        let mut session = CodexSession::connect_at(&addr.to_string(), &"a".repeat(64))
            .await
            .unwrap();
        let answer = session.turn("generate").await.unwrap();
        assert!(answer.contains("\"material_text\""));
        mock.await.unwrap();
    }
}
