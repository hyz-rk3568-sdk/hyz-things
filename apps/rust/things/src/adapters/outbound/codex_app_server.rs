//! Fixed-LAN Codex App Server adapter for the single-user study site.
//! Codex signs in once on the PC; this device only holds a capability token.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::{watch, Mutex},
};

pub const SERVER: &str = "192.168.8.202:1457";
const MAX_PACKET: usize = 256 * 1024;
const MAX_EVENTS: usize = 2048;
pub const TURN_TIMEOUT: Duration = Duration::from_secs(600);
const INTERRUPT_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const UNCONFIRMED_MESSAGE: &str = "未确认 Codex 远端已停止";
pub(super) const CANCELLED_MESSAGE: &str = "Codex 出题已取消（同批任务超时）";
type SharedWriter = Arc<Mutex<OwnedWriteHalf>>;

#[derive(Clone)]
enum TurnState {
    NotStarted,
    Starting,
    Running(String),
    Finished,
}

/// A fixed endpoint: never allow browser input or AI-generated content to select a host.
pub struct CodexSession {
    reader: OwnedReadHalf,
    writer: SharedWriter,
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
    // Codex is responsible for creating, checking and selecting the answers.
    // The App Server schema checks only the shape needed by the UI.
    let question = json!({
        "type":"object",
        "properties":{
            "question":{"type":"string"},
            "options":{"type":"array","items":{"type":"string"}},
            "correct_index":{"type":"integer"},
            "answer":{"type":"string"},
            "explanation":{"type":"string"},
            "skill":{"type":"string"},
            "source_kind":{"type":"string","enum":["wrong","knowledge"]},
            "target_id":{"type":"string"},
            "evidence":{"type":"string"}
        },
        "required":["question","options","correct_index","answer",
                    "explanation","skill","source_kind","target_id","evidence"],
        "additionalProperties":false
    });
    json!({
        "type":"object",
        "properties":{
            "material_text":{"type":"string"},
            "questions":{"type":"array","items":question}
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

async fn write_frame(
    socket: &mut (impl AsyncWrite + Unpin),
    opcode: u8,
    payload: &[u8],
) -> Result<(), String> {
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

async fn read_text(socket: &mut OwnedReadHalf, writer: &SharedWriter) -> Result<String, String> {
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
            0x9 => write_frame(&mut *writer.lock().await, 0xA, &payload).await?,
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

async fn send_message(writer: &SharedWriter, message: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(message).map_err(|_| "Codex JSON-RPC 编码失败")?;
    write_frame(&mut *writer.lock().await, 1, &bytes).await
}

impl CodexSession {
    pub async fn connect(token: &str, cwd: &str) -> Result<Self, String> {
        Self::connect_at(SERVER, token, cwd).await
    }

    async fn connect_at(address: &str, token: &str, cwd: &str) -> Result<Self, String> {
        validate_token(token)?;
        let mut socket = TcpStream::connect(address)
            .await
            .map_err(|_| "无法连接 192.168.8.202:1457；请检查电脑 Codex App Server")?;
        handshake(&mut socket, address, token).await?;
        let (reader, writer) = socket.into_split();
        let mut client = Self {
            reader,
            writer: Arc::new(Mutex::new(writer)),
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
                    "cwd":cwd,
                    "approvalPolicy":"never",
                    "sandbox":"danger-full-access",
                    "config":{"shell_environment_policy.set":{
                        "TMPDIR":"/tmp","TMP":"/tmp","TEMP":"/tmp"
                    }}
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
        send_message(&self.writer, message).await
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({"method":method,"params":params})).await
    }

    fn next_request_id(&mut self) -> i64 {
        self.request_id += 1;
        self.request_id
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_request_id();
        self.send(&json!({"id":id,"method":method,"params":params}))
            .await?;
        for _ in 0..MAX_EVENTS {
            let text = read_text(&mut self.reader, &self.writer).await?;
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
        self.turn_cancellable(prompt, watch::channel(false).0).await
    }

    pub async fn turn_cancellable(
        &mut self,
        prompt: &str,
        cancellation: watch::Sender<bool>,
    ) -> Result<String, String> {
        let mut cancelled = cancellation.subscribe();
        if *cancelled.borrow() {
            return Err(CANCELLED_MESSAGE.into());
        }
        let writer = Arc::clone(&self.writer);
        let thread = self.thread_id.clone();
        let interrupt_id = self.next_request_id();
        let (state_tx, mut state) = watch::channel(TurnState::NotStarted);
        let generation = self.run_turn(prompt, state_tx);
        tokio::pin!(generation);
        let error = tokio::select! {
            result = &mut generation => return result,
            _ = tokio::time::sleep(TURN_TIMEOUT) => {
                cancellation.send_replace(true);
                "Codex 出题超时（600 秒 / 10 分钟），本次出题已终止，请检查电脑服务与账号额度"
            }
            _ = cancelled.wait_for(|value| *value) => {
                CANCELLED_MESSAGE
            }
        };
        // Keep polling the same reader future so partial WebSocket frames survive
        // the deadline and a late turn/start response can still identify the turn.
        let cleanup = async {
            let turn_id = loop {
                match state.borrow().clone() {
                    TurnState::NotStarted | TurnState::Finished => return true,
                    TurnState::Running(id) => break id,
                    TurnState::Starting => {}
                }
                tokio::select! {
                    _ = &mut generation => return matches!(*state.borrow(), TurnState::Finished),
                    _ = state.changed() => {}
                }
            };
            let interrupt = json!({"id":interrupt_id,"method":"turn/interrupt",
                "params":{"threadId":thread,"turnId":turn_id}});
            tokio::select! {
                result = send_message(&writer, &interrupt) => {
                    if result.is_err() { return false; }
                }
                _ = &mut generation => return matches!(*state.borrow(), TurnState::Finished),
            }
            let _ = generation.await;
            matches!(*state.borrow(), TurnState::Finished)
        };
        if !matches!(
            tokio::time::timeout(INTERRUPT_TIMEOUT, cleanup).await,
            Ok(true)
        ) {
            tracing::warn!(thread_id = %thread, "Codex turn cancellation was not confirmed");
            return Err(format!("{error}；{UNCONFIRMED_MESSAGE}，请检查电脑服务"));
        }
        Err(error.into())
    }

    async fn run_turn(
        &mut self,
        prompt: &str,
        state: watch::Sender<TurnState>,
    ) -> Result<String, String> {
        state.send_replace(TurnState::Starting);
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
        state.send_replace(TurnState::Running(turn_id.clone()));
        let mut final_answer = String::new();
        for _ in 0..MAX_EVENTS {
            let raw = read_text(&mut self.reader, &self.writer).await?;
            let packet: Value =
                serde_json::from_str(&raw).map_err(|_| "Codex 返回非法 JSON-RPC 通知")?;
            // Reject interactive requests; responses such as interrupt acknowledgements
            // have an id without a method and need no reply.
            if let Some(id) = packet.get("id").filter(|_| packet.get("method").is_some()) {
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
                let status = packet
                    .pointer("/params/turn/status")
                    .and_then(Value::as_str);
                if !matches!(status, Some("completed" | "interrupted" | "failed")) {
                    return Err("Codex 返回未知的 Turn 完成状态".into());
                }
                state.send_replace(TurnState::Finished);
                if status != Some("completed") {
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
    fn generation_turn_deadline_is_ten_minutes() {
        assert_eq!(TURN_TIMEOUT, Duration::from_secs(600));
    }

    #[test]
    fn bearer_token_accepts_opaque_values_but_rejects_unsafe_headers() {
        assert!(validate_token("").is_err());
        assert!(validate_token("bad token").is_err());
        assert!(validate_token(&"x".repeat(64)).is_ok());
        assert!(validate_token(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn output_schema_accepts_complete_codex_questions() {
        let schema = output_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["properties"]["questions"]["items"]["properties"]["source_kind"]["enum"],
            json!(["wrong", "knowledge"])
        );
        assert!(schema["properties"]["questions"]["items"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "source_kind"));
        assert_eq!(
            schema["properties"]["questions"]["items"]["additionalProperties"],
            false
        );
        let fields = schema["properties"]["questions"]["items"]["required"]
            .as_array()
            .unwrap();
        assert_eq!(fields.len(), 9);
        assert!(fields.contains(&json!("target_id")));
        assert!(fields.contains(&json!("options")));
        assert!(fields.contains(&json!("correct_index")));
        assert!(schema["properties"]["questions"]["items"]["properties"]["operation"].is_null());
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

    async fn accept_session(listener: &TcpListener) -> TcpStream {
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
        assert_eq!(thread["params"]["cwd"], "/home/hyz/hyz_things");
        assert_eq!(thread["params"]["sandbox"], "danger-full-access");
        assert_eq!(
            thread["params"]["config"]["shell_environment_policy.set"],
            json!({"TMPDIR":"/tmp","TMP":"/tmp","TEMP":"/tmp"})
        );
        assert_eq!(thread["params"]["approvalPolicy"], "never");
        server_message(
            &mut socket,
            json!({"id":2,"result":{"thread":{"id":"test-thread"}}}),
        )
        .await;
        socket
    }

    async fn test_connection() -> (CodexSession, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let token = "a".repeat(64);
        let (session, socket) = tokio::join!(
            CodexSession::connect_at(&address, &token, "/home/hyz/hyz_things"),
            accept_session(&listener)
        );
        (session.unwrap(), socket)
    }

    async fn start_test_turn(socket: &mut TcpStream) {
        let turn = client_message(socket).await;
        assert_eq!(turn["method"], "turn/start");
        server_message(
            socket,
            json!({"id":turn["id"],"result":{"turn":{"id":"turn-1"}}}),
        )
        .await;
        // The reply proves the client has consumed turn/start before advancing time.
        server_message(
            socket,
            json!({"id":77,"method":"test/approval","params":{}}),
        )
        .await;
        let rejected = client_message(socket).await;
        assert_eq!(rejected["id"], 77);
        assert!(rejected.get("error").is_some());
    }

    async fn elapse(duration: Duration) {
        tokio::time::pause();
        tokio::time::advance(duration).await;
        // Real loopback I/O needs the runtime clock to resume after the deadline.
        tokio::time::resume();
    }

    #[tokio::test]
    async fn generation_timeout_interrupts_remote_turn() {
        let (mut session, mut socket) = test_connection().await;
        let task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(611), session.turn("generate")).await
        });
        start_test_turn(&mut socket).await;
        elapse(TURN_TIMEOUT).await;
        let interrupt = tokio::time::timeout(Duration::from_secs(1), client_message(&mut socket))
            .await
            .expect("deadline must send turn/interrupt before disconnecting");
        assert_eq!(interrupt["method"], "turn/interrupt");
        assert_eq!(
            interrupt["params"],
            json!({"threadId":"test-thread","turnId":"turn-1"})
        );
        server_message(&mut socket, json!({"id":interrupt["id"],"result":{}})).await;
        assert!(
            !task.is_finished(),
            "interrupt acknowledgement is not turn completion"
        );
        server_message(
            &mut socket,
            json!({"method":"turn/completed","params":{
                "threadId":"test-thread","turn":{"id":"turn-1","status":"interrupted"}
            }}),
        )
        .await;
        let error = task.await.unwrap().unwrap().unwrap_err();
        assert!(error.starts_with("Codex 出题超时"), "{error}");
        assert_eq!(
            socket.read(&mut [0u8; 1]).await.unwrap(),
            0,
            "interrupt responses must not trigger interactive-request replies"
        );
    }

    #[tokio::test]
    async fn batch_timeout_interrupts_peer_before_its_own_deadline() {
        let (mut first, mut first_socket) = test_connection().await;
        let (mut peer, mut peer_socket) = test_connection().await;
        let cancellation = watch::channel(false).0;
        let first_signal = cancellation.clone();
        let first_task =
            tokio::spawn(async move { first.turn_cancellable("first", first_signal).await });
        start_test_turn(&mut first_socket).await;
        elapse(TURN_TIMEOUT / 2).await;
        let peer_task =
            tokio::spawn(async move { peer.turn_cancellable("peer", cancellation).await });
        start_test_turn(&mut peer_socket).await;
        elapse(TURN_TIMEOUT / 2).await;
        for socket in [&mut first_socket, &mut peer_socket] {
            let interrupt = tokio::time::timeout(Duration::from_secs(1), client_message(socket))
                .await
                .expect("both running turns must be interrupted");
            assert_eq!(interrupt["method"], "turn/interrupt");
            server_message(socket, json!({"id":interrupt["id"],"result":{}})).await;
            complete_test_turn(socket, "interrupted").await;
        }
        assert!(first_task
            .await
            .unwrap()
            .unwrap_err()
            .starts_with("Codex 出题超时"));
        assert_eq!(peer_task.await.unwrap().unwrap_err(), CANCELLED_MESSAGE);
    }

    async fn complete_test_turn(socket: &mut TcpStream, status: &str) {
        server_message(
            socket,
            json!({"method":"turn/completed","params":{
                "threadId":"test-thread","turn":{"id":"turn-1","status":status}
            }}),
        )
        .await;
    }

    #[tokio::test]
    async fn interrupt_cleanup_is_bounded_when_server_never_confirms() {
        let (mut session, mut socket) = test_connection().await;
        let task = tokio::spawn(async move { session.turn("generate").await });
        start_test_turn(&mut socket).await;
        elapse(TURN_TIMEOUT).await;
        let interrupt = tokio::time::timeout(Duration::from_secs(1), client_message(&mut socket))
            .await
            .unwrap();
        assert_eq!(interrupt["method"], "turn/interrupt");
        server_message(&mut socket, json!({"id":interrupt["id"],"result":{}})).await;
        elapse(INTERRUPT_TIMEOUT).await;
        let error = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.starts_with("Codex 出题超时"));
        assert!(error.contains("未确认 Codex 远端已停止"));
    }

    #[tokio::test]
    async fn interrupt_requires_a_terminal_turn_status() {
        let (mut session, mut socket) = test_connection().await;
        let task = tokio::spawn(async move { session.turn("generate").await });
        start_test_turn(&mut socket).await;
        elapse(TURN_TIMEOUT).await;
        let interrupt = tokio::time::timeout(Duration::from_secs(1), client_message(&mut socket))
            .await
            .unwrap();
        assert_eq!(interrupt["method"], "turn/interrupt");
        complete_test_turn(&mut socket, "inProgress").await;
        let error = task.await.unwrap().unwrap_err();
        assert!(error.contains(UNCONFIRMED_MESSAGE), "{error}");
    }

    #[tokio::test]
    async fn cancelled_batch_does_not_start_a_new_turn() {
        let (mut session, mut socket) = test_connection().await;
        let cancellation = watch::channel(true).0;
        let error = session
            .turn_cancellable("generate", cancellation)
            .await
            .unwrap_err();
        assert_eq!(error, CANCELLED_MESSAGE);
        drop(session);
        assert_eq!(socket.read(&mut [0u8; 1]).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn late_turn_start_is_interrupted_after_deadline() {
        let (mut session, mut socket) = test_connection().await;
        let task = tokio::spawn(async move { session.turn("generate").await });
        let start = client_message(&mut socket).await;
        assert_eq!(start["method"], "turn/start");
        elapse(TURN_TIMEOUT).await;
        server_message(
            &mut socket,
            json!({"id":start["id"],"result":{"turn":{"id":"turn-1"}}}),
        )
        .await;
        let interrupt = tokio::time::timeout(Duration::from_secs(1), client_message(&mut socket))
            .await
            .expect("late turn id must still be cancelled");
        assert_eq!(interrupt["method"], "turn/interrupt");
        assert_eq!(interrupt["params"]["turnId"], "turn-1");
        complete_test_turn(&mut socket, "interrupted").await;
        let error = task.await.unwrap().unwrap_err();
        assert!(error.starts_with("Codex 出题超时"));
        assert!(!error.contains("未确认"));
    }

    #[tokio::test]
    async fn interrupt_preserves_a_partially_received_websocket_frame() {
        let (mut session, mut socket) = test_connection().await;
        let task = tokio::spawn(async move { session.turn("generate").await });
        start_test_turn(&mut socket).await;
        let event = serde_json::to_vec(&json!({"method":"test/event","params":{}})).unwrap();
        socket.write_all(&[0x81, event.len() as u8]).await.unwrap();
        socket.write_all(&event[..1]).await.unwrap();
        elapse(TURN_TIMEOUT).await;
        let interrupt = tokio::time::timeout(Duration::from_secs(1), client_message(&mut socket))
            .await
            .unwrap();
        assert_eq!(interrupt["method"], "turn/interrupt");
        socket.write_all(&event[1..]).await.unwrap();
        complete_test_turn(&mut socket, "interrupted").await;
        let error = task.await.unwrap().unwrap_err();
        assert!(error.starts_with("Codex 出题超时"));
        assert!(!error.contains("未确认"));
    }

    #[tokio::test]
    async fn fake_codex_app_server_completes_generation_without_external_network() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mock = tokio::spawn(async move {
            let mut socket = accept_session(&listener).await;
            let turn = client_message(&mut socket).await;
            assert_eq!(turn["method"], "turn/start");
            assert_eq!(turn["params"]["outputSchema"]["type"], "object");
            server_message(
                &mut socket,
                json!({"id":turn["id"],"result":{"turn":{"id":"turn-1"}}}),
            )
            .await;
            // Tool execution reports as events, without interactive approval.
            server_message(
                &mut socket,
                json!({"method":"item/completed","params":{
                    "threadId":"test-thread","turnId":"turn-1",
                    "item":{"type":"commandExecution","command":"pwd","exitCode":0}
                }}),
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
        let mut session =
            CodexSession::connect_at(&addr.to_string(), &"a".repeat(64), "/home/hyz/hyz_things")
                .await
                .unwrap();
        let answer = session.turn("generate").await.unwrap();
        assert!(answer.contains("\"material_text\""));
        mock.await.unwrap();
    }
}
