//! Fixed-LAN Codex App Server adapter for the single-user study site.
//! Codex signs in once on the PC; this device only holds a capability token.
use crate::application::question_bank::practice::{AiReviewEventSink, AiReviewProgress};
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
const MAX_PACKET: usize = 512 * 1024;
const MAX_INIT_EVENTS: usize = 2048; // Only the initialization RPC; generation is time-bounded.
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
    /// Resumed threads already exist with an older sandbox policy; the next
    /// turn must request full access explicitly.
    resumed: bool,
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

fn review_answer(raw: &str) -> Option<String> {
    let answer = raw.trim();
    (!answer.is_empty() && answer.len() <= MAX_PACKET).then(|| answer.to_owned())
}

#[derive(Default)]
struct ReviewEventMapper {
    items: std::collections::HashMap<String, String>,
}

impl ReviewEventMapper {
    fn map(&mut self, packet: &Value, thread: &str, turn: &str) -> Option<AiReviewProgress> {
        let params = packet.get("params")?;
        if params.get("threadId")?.as_str()? != thread || params.get("turnId")?.as_str()? != turn {
            return None;
        }
        let method = packet.get("method")?.as_str()?;
        if method == "item/started" {
            let item = params.get("item")?;
            let id = item.get("id")?.as_str()?;
            let kind = item.get("type")?.as_str()?;
            if id.is_empty()
                || self.items.contains_key(id)
                || !matches!(
                    kind,
                    "agentMessage"
                        | "reasoning"
                        | "commandExecution"
                        | "fileChange"
                        | "mcpToolCall"
                        | "webSearch"
                )
            {
                return None;
            }
            if id.len() > 256 || self.items.len() >= 256 {
                return Some(AiReviewProgress::LimitExceeded);
            }
            self.items.insert(id.to_owned(), kind.to_owned());
            if matches!(
                kind,
                "commandExecution" | "fileChange" | "mcpToolCall" | "webSearch"
            ) {
                return Some(AiReviewProgress::ToolStarted {
                    item_id: id.to_owned(),
                    tool: kind.to_owned(),
                    details: tool_details(item, false),
                });
            }
        } else if method == "item/completed" {
            let item = params.get("item")?;
            let id = item.get("id")?.as_str()?;
            let kind = self.items.remove(id)?;
            if matches!(
                kind.as_str(),
                "commandExecution" | "fileChange" | "mcpToolCall" | "webSearch"
            ) {
                let status = item
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                if status.len() > 64 {
                    return Some(AiReviewProgress::LimitExceeded);
                }
                return Some(AiReviewProgress::ToolFinished {
                    item_id: id.to_owned(),
                    tool: kind,
                    status: status.to_owned(),
                    details: tool_details(item, true),
                });
            }
        } else if method == "item/commandExecution/outputDelta" {
            let id = params.get("itemId")?.as_str()?;
            if self.items.get(id)?.as_str() != "commandExecution" {
                return None;
            }
            let text = params.get("delta")?.as_str()?;
            if !text.is_empty() {
                return Some(AiReviewProgress::CommandOutputDelta {
                    item_id: id.to_owned(),
                    text: text.to_owned(),
                });
            }
        } else if method == "item/fileChange/outputDelta" {
            let id = params.get("itemId")?.as_str()?;
            if self.items.get(id)?.as_str() != "fileChange" {
                return None;
            }
            let text = params.get("delta")?.as_str()?;
            if !text.is_empty() {
                return Some(AiReviewProgress::ToolOutputDelta {
                    item_id: id.to_owned(),
                    tool: "fileChange".to_owned(),
                    text: text.to_owned(),
                });
            }
        } else if method == "item/mcpToolCall/progress" {
            let id = params.get("itemId")?.as_str()?;
            if self.items.get(id)?.as_str() != "mcpToolCall" {
                return None;
            }
            let message = params.get("message")?.as_str()?;
            if !message.is_empty() {
                return Some(AiReviewProgress::ToolProgress {
                    item_id: id.to_owned(),
                    tool: "mcpToolCall".to_owned(),
                    message: message.to_owned(),
                });
            }
        } else if matches!(
            method,
            "item/agentMessage/delta" | "item/reasoning/summaryTextDelta"
        ) {
            let id = params.get("itemId")?.as_str()?;
            let kind = self.items.get(id)?;
            let text = params.get("delta")?.as_str()?;
            if text.is_empty() {
                return None;
            }
            if method == "item/agentMessage/delta" && kind == "agentMessage" {
                return Some(AiReviewProgress::TextDelta {
                    item_id: id.to_owned(),
                    text: text.to_owned(),
                });
            }
            if method == "item/reasoning/summaryTextDelta" && kind == "reasoning" {
                return Some(AiReviewProgress::ReasoningSummaryDelta {
                    item_id: id.to_owned(),
                    text: text.to_owned(),
                });
            }
        }
        None
    }
}

const OUTPUT_CHUNK_BYTES: usize = 16 * 1024;

async fn send_review_progress(
    sink: &AiReviewEventSink,
    event: AiReviewProgress,
) -> Result<(), String> {
    match event {
        AiReviewProgress::CommandOutputDelta { item_id, text } => {
            send_output_chunks(sink, &text, |text| AiReviewProgress::CommandOutputDelta {
                item_id: item_id.clone(),
                text,
            })
            .await
        }
        AiReviewProgress::ToolOutputDelta {
            item_id,
            tool,
            text,
        } => {
            send_output_chunks(sink, &text, |text| AiReviewProgress::ToolOutputDelta {
                item_id: item_id.clone(),
                tool: tool.clone(),
                text,
            })
            .await
        }
        other => sink(other).await,
    }
}

async fn send_output_chunks(
    sink: &AiReviewEventSink,
    text: &str,
    make_event: impl Fn(String) -> AiReviewProgress,
) -> Result<(), String> {
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + OUTPUT_CHUNK_BYTES).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        sink(make_event(text[start..end].to_owned())).await?;
        start = end;
    }
    Ok(())
}

fn tool_details(item: &Value, completed: bool) -> Value {
    let mut details = serde_json::Map::new();
    let fields: &[&str] = if completed {
        &[
            "command",
            "cwd",
            "exitCode",
            "durationMs",
            "aggregatedOutput",
            "server",
            "tool",
            "arguments",
            "result",
            "error",
            "changes",
            "query",
        ]
    } else {
        &[
            "command",
            "cwd",
            "server",
            "tool",
            "arguments",
            "changes",
            "query",
        ]
    };
    for field in fields {
        if let Some(value) = item.get(*field) {
            details.insert((*field).to_owned(), value.clone());
        }
    }
    Value::Object(details)
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

async fn read_payload(socket: &mut OwnedReadHalf, length: usize) -> Result<Vec<u8>, String> {
    let mut payload = vec![0u8; length];
    socket
        .read_exact(&mut payload)
        .await
        .map_err(|_| "Codex 帧读取失败".to_owned())?;
    Ok(payload)
}

async fn discard_payload(socket: &mut OwnedReadHalf, mut length: usize) -> Result<(), String> {
    let mut buffer = [0u8; 8 * 1024];
    while length > 0 {
        let chunk = length.min(buffer.len());
        socket
            .read_exact(&mut buffer[..chunk])
            .await
            .map_err(|_| "Codex 帧读取失败".to_owned())?;
        length -= chunk;
    }
    Ok(())
}

async fn append_payload(
    socket: &mut OwnedReadHalf,
    message: &mut Vec<u8>,
    mut length: usize,
) -> Result<(), String> {
    let mut chunk = [0u8; 16 * 1024];
    while length > 0 {
        let count = length.min(chunk.len());
        socket
            .read_exact(&mut chunk[..count])
            .await
            .map_err(|_| "Codex 帧读取失败")?;
        message
            .try_reserve(count)
            .map_err(|_| "设备内存不足，无法接收 Codex 输出")?;
        message.extend_from_slice(&chunk[..count]);
        length -= count;
    }
    Ok(())
}

async fn read_text(socket: &mut OwnedReadHalf, writer: &SharedWriter) -> Result<String, String> {
    read_text_bounded(socket, writer, false).await
}

async fn read_text_bounded(
    socket: &mut OwnedReadHalf,
    writer: &SharedWriter,
    strict: bool,
) -> Result<String, String> {
    let mut message = Vec::new();
    let mut fragmented = false;
    let mut discarding = false;
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
        let length = usize::try_from(length).map_err(|_| "Codex 帧长度无效")?;
        if opcode >= 8 {
            if !final_frame || length > 125 {
                return Err("Codex WebSocket 控制帧无效".into());
            }
            let payload = read_payload(socket, length).await?;
            match opcode {
                0x8 => return Err("Codex App Server 已关闭连接".into()),
                0x9 => write_frame(&mut *writer.lock().await, 0xA, &payload).await?,
                0xA => {}
                _ => return Err("Codex WebSocket 收到不支持的数据帧".into()),
            }
            continue;
        }
        match opcode {
            0x1 if !fragmented && !discarding => {
                if !strict && length > MAX_PACKET {
                    discard_payload(socket, length).await?;
                    discarding = !final_frame;
                } else {
                    append_payload(socket, &mut message, length).await?;
                    if final_frame {
                        return String::from_utf8(message)
                            .map_err(|_| "Codex 响应不是 UTF-8".into());
                    }
                    fragmented = true;
                }
            }
            0x0 if fragmented || discarding => {
                if discarding {
                    discard_payload(socket, length).await?;
                    if final_frame {
                        discarding = false;
                    }
                } else if !strict && message.len().saturating_add(length) > MAX_PACKET {
                    message.clear();
                    fragmented = false;
                    discard_payload(socket, length).await?;
                    discarding = !final_frame;
                } else {
                    append_payload(socket, &mut message, length).await?;
                    if final_frame {
                        return String::from_utf8(message)
                            .map_err(|_| "Codex 响应不是 UTF-8".into());
                    }
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

    #[cfg(test)]
    pub(super) async fn connect_test_server(
        address: std::net::SocketAddr,
        token: &str,
        cwd: &str,
    ) -> Result<Self, String> {
        Self::connect_at(&address.to_string(), token, cwd).await
    }

    /// Persistent study threads are separate from ephemeral generation threads;
    /// both run with the maximum sandbox access requested by the product owner.
    pub async fn connect_review(
        token: &str,
        cwd: &str,
        previous_thread: Option<&str>,
    ) -> Result<Self, String> {
        Self::connect_at_mode(SERVER, token, cwd, previous_thread, true, false).await
    }

    #[cfg(test)]
    pub(super) async fn connect_review_test_server(
        address: std::net::SocketAddr,
        token: &str,
        cwd: &str,
        previous_thread: Option<&str>,
    ) -> Result<Self, String> {
        Self::connect_at_mode(
            &address.to_string(),
            token,
            cwd,
            previous_thread,
            true,
            false,
        )
        .await
    }

    /// 申论使用独立临时线程，但与现有 Codex 任务一样允许全部权限。
    /// 试卷、图片和学生答案始终视为不可信输入；权限配置由设备所有者选择。
    pub async fn connect_shenlun(token: &str, cwd: &str) -> Result<Self, String> {
        Self::connect_at_mode(SERVER, token, cwd, None, false, false).await
    }

    #[cfg(test)]
    pub(super) async fn connect_shenlun_test_server(
        address: std::net::SocketAddr,
        token: &str,
        cwd: &str,
    ) -> Result<Self, String> {
        Self::connect_at_mode(&address.to_string(), token, cwd, None, false, false).await
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    async fn connect_at(address: &str, token: &str, cwd: &str) -> Result<Self, String> {
        Self::connect_at_mode(address, token, cwd, None, false, false).await
    }

    async fn connect_at_mode(
        address: &str,
        token: &str,
        cwd: &str,
        previous_thread: Option<&str>,
        persistent: bool,
        restricted: bool,
    ) -> Result<Self, String> {
        let mut client = Self::initialize_at(address, token).await?;
        client.resumed = previous_thread.is_some();
        let thread = if let Some(thread_id) = previous_thread {
            client
                .request(
                    "thread/resume",
                    json!({"threadId":thread_id,"excludeTurns":true}),
                )
                .await?
        } else {
            client
                .request(
                    "thread/start",
                    json!({
                        "ephemeral":!persistent,
                        "cwd":cwd,
                        "approvalPolicy":"never",
                        "sandbox":if restricted {"read-only"} else {"danger-full-access"},
                        "config":{"shell_environment_policy.set":{
                            "TMPDIR":"/tmp","TMP":"/tmp","TEMP":"/tmp"
                        }}
                    }),
                )
                .await?
        };
        client.thread_id = thread
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or("Codex 未返回有效 Thread ID")?
            .to_owned();
        Ok(client)
    }

    async fn initialize_at(address: &str, token: &str) -> Result<Self, String> {
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
            resumed: false,
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
        Ok(client)
    }

    pub async fn rate_limits(token: &str) -> Result<Value, String> {
        let mut client = Self::initialize_at(SERVER, token).await?;
        client.request("account/rateLimits/read", json!({})).await
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
        for _ in 0..MAX_INIT_EVENTS {
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

    /// Return one independently reasoned AI review message as Markdown.
    pub async fn turn_review(
        &mut self,
        prompt: &str,
        images: &[String],
        cancellation: watch::Sender<bool>,
    ) -> Result<String, String> {
        let raw = self
            .turn_cancellable_inner(prompt, images, cancellation, None, None)
            .await?;
        review_answer(&raw).ok_or_else(|| "Codex AI 解析内容为空或过长".to_owned())
    }

    pub async fn turn_review_stream(
        &mut self,
        prompt: &str,
        images: &[String],
        cancellation: watch::Sender<bool>,
        sink: AiReviewEventSink,
    ) -> Result<String, String> {
        let raw = self
            .turn_cancellable_inner(prompt, images, cancellation, None, Some(sink))
            .await?;
        review_answer(&raw).ok_or_else(|| "Codex AI 解析内容为空或过长".to_owned())
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
        self.turn_cancellable_with_schema(prompt, &[], cancellation, output_schema())
            .await
    }

    pub async fn turn_with_schema(
        &mut self,
        prompt: &str,
        schema: Value,
    ) -> Result<String, String> {
        self.turn_cancellable_with_schema(prompt, &[], watch::channel(false).0, schema)
            .await
    }

    pub async fn turn_cancellable_with_schema(
        &mut self,
        prompt: &str,
        images: &[String],
        cancellation: watch::Sender<bool>,
        schema: Value,
    ) -> Result<String, String> {
        self.turn_cancellable_inner(prompt, images, cancellation, Some(schema), None)
            .await
    }

    /// Existing 4x5 data-analysis generation schema, with the same tool stream.
    pub async fn turn_cancellable_stream(
        &mut self,
        prompt: &str,
        cancellation: watch::Sender<bool>,
        sink: AiReviewEventSink,
    ) -> Result<String, String> {
        self.turn_cancellable_inner(prompt, &[], cancellation, Some(output_schema()), Some(sink))
            .await
    }

    /// Same Codex JSON Schema contract as other generation turns, with
    /// tool execution activity forwarded to a transient preparation sink.
    pub async fn turn_cancellable_with_schema_stream(
        &mut self,
        prompt: &str,
        images: &[String],
        cancellation: watch::Sender<bool>,
        schema: Value,
        sink: AiReviewEventSink,
    ) -> Result<String, String> {
        self.turn_cancellable_inner(prompt, images, cancellation, Some(schema), Some(sink))
            .await
    }

    async fn turn_cancellable_inner(
        &mut self,
        prompt: &str,
        images: &[String],
        cancellation: watch::Sender<bool>,
        schema: Option<Value>,
        sink: Option<AiReviewEventSink>,
    ) -> Result<String, String> {
        let mut cancelled = cancellation.subscribe();
        if *cancelled.borrow() {
            return Err(CANCELLED_MESSAGE.into());
        }
        let writer = Arc::clone(&self.writer);
        let thread = self.thread_id.clone();
        let interrupt_id = self.next_request_id();
        let (state_tx, mut state) = watch::channel(TurnState::NotStarted);
        let generation = self.run_turn(prompt, images, state_tx, schema, sink);
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
        images: &[String],
        state: watch::Sender<TurnState>,
        schema: Option<Value>,
        sink: Option<AiReviewEventSink>,
    ) -> Result<String, String> {
        state.send_replace(TurnState::Starting);
        let thread = self.thread_id.clone();
        let mut input = vec![json!({"type":"text","text":prompt})];
        input.extend(
            images
                .iter()
                .map(|path| json!({"type":"localImage","path":path})),
        );
        let mut params = json!({
            "threadId":thread,
            "input":input
        });
        if let Some(schema) = schema {
            params["outputSchema"] = schema;
        }
        // Resumed threads may still carry the old read-only policy; request the
        // maximum sandbox explicitly for every turn in that thread.
        if self.resumed {
            params["sandboxPolicy"] = json!({"type":"dangerFullAccess"});
        }
        let request_id = self.next_request_id();
        self.send(&json!({"id":request_id,"method":"turn/start","params":params}))
            .await?;
        let mut pending = std::collections::VecDeque::new();
        let mut pending_bytes = 0usize;
        let result = loop {
            let raw = read_text_bounded(&mut self.reader, &self.writer, sink.is_some()).await?;
            let packet: Value =
                serde_json::from_str(&raw).map_err(|_| "Codex 返回非法 JSON-RPC")?;
            if packet.get("id").and_then(Value::as_i64) == Some(request_id)
                && packet.get("method").is_none()
            {
                if packet.get("error").is_some() {
                    return Err("Codex 拒绝启动 Turn".into());
                }
                break packet
                    .get("result")
                    .cloned()
                    .ok_or("Codex 响应缺少 result")?;
            }
            if packet.get("id").is_some() && packet.get("method").is_some() {
                let id = &packet["id"];
                self.send(&json!({"id":id,"error":{"code":-32601,"message":"Interactive tools unavailable"}})).await?;
            } else if packet.get("method").is_some() {
                pending_bytes = pending_bytes.saturating_add(raw.len());
                if pending.len() >= 128 || pending_bytes > MAX_PACKET {
                    return Err("Codex 启动通知过多，远端状态未确认".into());
                }
                pending.push_back(packet);
            }
        };
        let turn_id = result
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .ok_or("Codex 未返回有效 Turn ID")?
            .to_owned();
        state.send_replace(TurnState::Running(turn_id.clone()));
        let mut final_answer = String::new();
        let mut mapper = ReviewEventMapper::default();
        // A generation turn is bounded by TURN_TIMEOUT, not by notification count.
        // A long but healthy reasoning stream may exceed 2048 notifications.
        loop {
            let packet: Value = if let Some(packet) = pending.pop_front() {
                packet
            } else {
                let raw = read_text_bounded(&mut self.reader, &self.writer, sink.is_some()).await?;
                serde_json::from_str(&raw).map_err(|_| "Codex 返回非法 JSON-RPC 通知")?
            };
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
            if let Some(sink) = &sink {
                if let Some(event) = mapper.map(&packet, &thread, &turn_id) {
                    send_review_progress(sink, event).await?;
                }
            }
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn review_progress_requires_matching_turn_and_started_agent_item() {
        let mut mapper = ReviewEventMapper::default();
        let delta = |thread: &str, turn: &str| {
            json!({"method":"item/agentMessage/delta", "params":{
                "threadId":thread,"turnId":turn,"itemId":"item-1","delta":"正文"
            }})
        };
        assert!(mapper
            .map(&delta("thread", "turn"), "thread", "turn")
            .is_none());
        let started = json!({"method":"item/started","params":{"threadId":"thread","turnId":"turn",
            "item":{"id":"item-1","type":"agentMessage"}}});
        assert!(mapper.map(&started, "thread", "turn").is_none());
        assert!(mapper
            .map(&delta("other", "turn"), "thread", "turn")
            .is_none());
        assert!(mapper
            .map(&delta("thread", "other"), "thread", "turn")
            .is_none());
        assert!(matches!(
            mapper.map(&delta("thread", "turn"), "thread", "turn"),
            Some(AiReviewProgress::TextDelta { .. })
        ));
    }

    #[test]
    fn review_tool_events_pair_only_matching_started_items() {
        let mut mapper = ReviewEventMapper::default();
        let started = json!({"method":"item/started","params":{"threadId":"t","turnId":"r",
            "item":{"id":"tool","type":"commandExecution"}}});
        let finished = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"tool","type":"commandExecution","status":"failed"}}});
        assert!(mapper.map(&finished, "t", "r").is_none());
        assert!(matches!(
            mapper.map(&started, "t", "r"),
            Some(AiReviewProgress::ToolStarted { .. })
        ));
        assert!(
            matches!(mapper.map(&finished, "t", "r"), Some(AiReviewProgress::ToolFinished { status, .. }) if status == "failed")
        );
        assert!(mapper.map(&finished, "t", "r").is_none());
    }

    #[test]
    fn command_output_is_scoped_to_active_command_and_turn() {
        let mut mapper = ReviewEventMapper::default();
        let started = json!({"method":"item/started","params":{"threadId":"t","turnId":"r",
            "item":{"id":"cmd","type":"commandExecution"}}});
        let delta = json!({"method":"item/commandExecution/outputDelta","params":{
            "threadId":"t","turnId":"r","itemId":"cmd","delta":"hello"}});
        assert!(mapper.map(&delta, "t", "r").is_none());
        mapper.map(&started, "t", "r");
        assert!(mapper.map(&delta, "t", "other").is_none());
        assert!(matches!(mapper.map(&delta, "t", "r"),
            Some(AiReviewProgress::CommandOutputDelta { item_id, text })
                if item_id == "cmd" && text == "hello"));
        let completed = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"cmd","type":"commandExecution"}}});
        mapper.map(&completed, "t", "r");
        assert!(mapper.map(&delta, "t", "r").is_none());
    }

    #[test]
    fn command_lifecycle_preserves_command_and_exit_details() {
        let mut mapper = ReviewEventMapper::default();
        let started = json!({"method":"item/started","params":{"threadId":"t","turnId":"r",
            "item":{"id":"cmd","type":"commandExecution","command":"pwd","cwd":"/work"}}});
        let completed = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"cmd","type":"commandExecution","status":"completed","exitCode":0}}});
        assert!(matches!(mapper.map(&started, "t", "r"),
            Some(AiReviewProgress::ToolStarted { details, .. }) if details["command"] == "pwd"));
        assert!(matches!(mapper.map(&completed, "t", "r"),
            Some(AiReviewProgress::ToolFinished { details, .. }) if details["exitCode"] == 0));
    }

    #[test]
    fn file_and_mcp_progress_require_the_matching_live_item() {
        let mut mapper = ReviewEventMapper::default();
        let started = |id: &str, kind: &str| {
            json!({"method":"item/started","params":{
            "threadId":"t","turnId":"r","item":{"id":id,"type":kind}}})
        };
        let file = json!({"method":"item/fileChange/outputDelta","params":{
            "threadId":"t","turnId":"r","itemId":"f","delta":"patch output"}});
        let mcp = json!({"method":"item/mcpToolCall/progress","params":{
            "threadId":"t","turnId":"r","itemId":"m","message":"working"}});
        assert!(mapper.map(&file, "t", "r").is_none());
        mapper.map(&started("f", "fileChange"), "t", "r");
        mapper.map(&started("m", "mcpToolCall"), "t", "r");
        assert!(mapper.map(&file, "other", "r").is_none());
        assert!(mapper.map(&mcp, "t", "other").is_none());
        assert!(matches!(mapper.map(&file, "t", "r"),
            Some(AiReviewProgress::ToolOutputDelta { tool, text, .. })
                if tool == "fileChange" && text == "patch output"));
        assert!(matches!(mapper.map(&mcp, "t", "r"),
            Some(AiReviewProgress::ToolProgress { tool, message, .. })
                if tool == "mcpToolCall" && message == "working"));
        let wrong = json!({"method":"item/fileChange/outputDelta","params":{
            "threadId":"t","turnId":"r","itemId":"m","delta":"wrong"}});
        assert!(mapper.map(&wrong, "t", "r").is_none());
        let complete = json!({"method":"item/completed","params":{"threadId":"t","turnId":"r",
            "item":{"id":"m","type":"mcpToolCall","status":"completed"}}});
        mapper.map(&complete, "t", "r");
        assert!(mapper.map(&mcp, "t", "r").is_none());
    }

    #[tokio::test]
    async fn tool_output_chunks_preserve_utf8_and_order() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sink: AiReviewEventSink = Arc::new(move |event| {
            let tx = tx.clone();
            Box::pin(async move { tx.send(event).map_err(|_| "closed".to_owned()) })
        });
        let text = "题".repeat(OUTPUT_CHUNK_BYTES);
        send_review_progress(
            &sink,
            AiReviewProgress::CommandOutputDelta {
                item_id: "cmd".into(),
                text: text.clone(),
            },
        )
        .await
        .unwrap();
        let mut reconstructed = String::new();
        while let Ok(event) = rx.try_recv() {
            match event {
                AiReviewProgress::CommandOutputDelta { item_id, text } => {
                    assert_eq!(item_id, "cmd");
                    assert!(text.len() <= OUTPUT_CHUNK_BYTES);
                    reconstructed.push_str(&text);
                }
                _ => panic!("unexpected event"),
            }
        }
        assert_eq!(reconstructed, text);
    }

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

    async fn server_frame(socket: &mut TcpStream, fin: bool, opcode: u8, payload: &[u8]) {
        let first = (if fin { 0x80 } else { 0 }) | opcode;
        let length = payload.len();
        if length < 126 {
            socket.write_all(&[first, length as u8]).await.unwrap();
        } else if length <= u16::MAX as usize {
            socket.write_all(&[first, 126]).await.unwrap();
            socket
                .write_all(&(length as u16).to_be_bytes())
                .await
                .unwrap();
        } else {
            socket.write_all(&[first, 127]).await.unwrap();
            socket
                .write_all(&(length as u64).to_be_bytes())
                .await
                .unwrap();
        }
        socket.write_all(payload).await.unwrap();
    }

    async fn server_large_message(socket: &mut TcpStream, msg: &Value) {
        let bytes = serde_json::to_vec(msg).unwrap();
        server_frame(socket, true, 0x1, &bytes).await;
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
        let (socket, thread) = accept_session_expecting(listener, None).await;
        assert_eq!(thread["method"], "thread/start");
        assert_eq!(thread["params"]["ephemeral"], true);
        assert_eq!(thread["params"]["cwd"], "/home/hyz/hyz_things");
        assert_eq!(thread["params"]["sandbox"], "danger-full-access");
        assert_eq!(
            thread["params"]["config"]["shell_environment_policy.set"],
            json!({"TMPDIR":"/tmp","TMP":"/tmp","TEMP":"/tmp"})
        );
        assert_eq!(thread["params"]["approvalPolicy"], "never");
        socket
    }

    async fn accept_session_expecting(
        listener: &TcpListener,
        previous_thread: Option<&str>,
    ) -> (TcpStream, Value) {
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
        let reply = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
            accept_key(key)
        );
        socket.write_all(reply.as_bytes()).await.unwrap();
        let init = client_message(&mut socket).await;
        assert_eq!(init["method"], "initialize");
        server_message(&mut socket, json!({"id":1,"result":{}})).await;
        let initialized = client_message(&mut socket).await;
        assert_eq!(initialized["method"], "initialized");
        let thread = client_message(&mut socket).await;
        let thread_id = previous_thread.unwrap_or("test-thread");
        server_message(
            &mut socket,
            json!({"id":2,"result":{"thread":{"id":thread_id}}}),
        )
        .await;
        (socket, thread)
    }

    async fn accept_review(
        listener: &TcpListener,
        previous_thread: Option<&str>,
    ) -> (CodexSession, TcpStream, Value) {
        let address = listener.local_addr().unwrap();
        let token = "a".repeat(64);
        let (session, server) = tokio::join!(
            CodexSession::connect_review_test_server(
                address,
                &token,
                "/home/hyz/hyz_things",
                previous_thread,
            ),
            accept_session_expecting(listener, previous_thread),
        );
        let (socket, thread) = server;
        (session.unwrap(), socket, thread)
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

    #[tokio::test]
    async fn review_thread_is_persistent_full_access_and_reports_its_id() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (session, _socket, thread) = accept_review(&listener, None).await;
        assert_eq!(thread["method"], "thread/start");
        assert_eq!(thread["params"]["ephemeral"], false);
        assert_eq!(thread["params"]["sandbox"], "danger-full-access");
        assert_eq!(thread["params"]["approvalPolicy"], "never");
        assert_eq!(thread["params"]["cwd"], "/home/hyz/hyz_things");
        assert_eq!(session.thread_id(), "test-thread");
    }

    #[tokio::test]
    async fn review_follow_up_resumes_the_existing_thread_without_replaying_turns() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (session, _socket, thread) = accept_review(&listener, Some("previous-thread")).await;
        assert_eq!(thread["method"], "thread/resume");
        assert_eq!(thread["params"]["threadId"], "previous-thread");
        assert_eq!(thread["params"]["excludeTurns"], true);
        assert!(thread["params"].get("sandbox").is_none());
        assert_eq!(session.thread_id(), "previous-thread");
    }

    #[tokio::test]
    async fn resumed_thread_requests_full_access_for_the_next_turn() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (mut session, mut socket, _thread) =
            accept_review(&listener, Some("previous-thread")).await;
        let task = tokio::spawn(async move {
            session
                .turn_review("review", &[], watch::channel(false).0)
                .await
        });
        let request = client_message(&mut socket).await;
        assert_eq!(request["method"], "turn/start");
        assert_eq!(
            request["params"]["sandboxPolicy"],
            json!({"type":"dangerFullAccess"})
        );
        task.abort();
    }

    #[tokio::test]
    async fn review_turn_sends_uploaded_images_as_local_image_inputs() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (mut session, mut socket, _thread) = accept_review(&listener, None).await;
        let images = vec![
            "/home/hyz/hyz_things/tmp/hyz-ai-uploads/one.png".to_owned(),
            "/home/hyz/hyz_things/tmp/hyz-ai-uploads/two.jpg".to_owned(),
        ];
        let task = tokio::spawn(async move {
            session
                .turn_review("check the handwriting", &images, watch::channel(false).0)
                .await
        });
        let request = client_message(&mut socket).await;
        assert_eq!(request["method"], "turn/start");
        assert_eq!(
            request["params"]["input"],
            json!([
                {"type":"text","text":"check the handwriting"},
                {"type":"localImage","path":"/home/hyz/hyz_things/tmp/hyz-ai-uploads/one.png"},
                {"type":"localImage","path":"/home/hyz/hyz_things/tmp/hyz-ai-uploads/two.jpg"}
            ])
        );
        assert!(request["params"].get("sandboxPolicy").is_none());
        task.abort();
    }

    #[tokio::test]
    async fn review_stream_preserves_matching_notifications_before_start_response() {
        let (mut session, mut socket) = test_connection().await;
        let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
        let sink: AiReviewEventSink = Arc::new(move |event| {
            let tx = tx.clone();
            Box::pin(async move { tx.send(event).map_err(|_| "closed".to_owned()) })
        });
        let task = tokio::spawn(async move {
            session
                .turn_review_stream("review", &[], watch::channel(false).0, sink)
                .await
        });
        let request = client_message(&mut socket).await;
        server_message(&mut socket, json!({"method":"item/started","params":{"threadId":"test-thread","turnId":"turn-1","item":{"id":"a","type":"agentMessage"}}})).await;
        server_message(&mut socket, json!({"method":"item/agentMessage/delta","params":{"threadId":"test-thread","turnId":"turn-1","itemId":"a","delta":"正文"}})).await;
        server_message(
            &mut socket,
            json!({"id":request["id"],"result":{"turn":{"id":"turn-1"}}}),
        )
        .await;
        server_message(&mut socket, json!({"method":"item/completed","params":{"threadId":"test-thread","turnId":"turn-1","item":{"id":"a","type":"agentMessage","text":"正文完成"}}})).await;
        complete_test_turn(&mut socket, "completed").await;
        assert!(
            matches!(events.recv().await, Some(AiReviewProgress::TextDelta { text, .. }) if text == "正文")
        );
        assert_eq!(task.await.unwrap().unwrap(), "正文完成");
    }

    #[tokio::test]
    async fn review_requests_and_accepts_plain_markdown() {
        let (mut session, mut socket) = test_connection().await;
        let task = tokio::spawn(async move {
            session
                .turn_review("review", &[], watch::channel(false).0)
                .await
        });
        let request = client_message(&mut socket).await;
        assert!(request["params"].get("outputSchema").is_none());
        server_message(
            &mut socket,
            json!({"id":request["id"],"result":{"turn":{"id":"turn-1"}}}),
        )
        .await;
        server_message(
            &mut socket,
            json!({"method":"item/completed","params":{"threadId":"test-thread","turnId":"turn-1",
                "item":{"type":"agentMessage","text":"## 直接输出的解析\n\n```svg\n<svg viewBox=\"0 0 10 10\"/>\n```"}}}),
        )
        .await;
        complete_test_turn(&mut socket, "completed").await;
        assert_eq!(
            task.await.unwrap().unwrap(),
            "## 直接输出的解析\n\n```svg\n<svg viewBox=\"0 0 10 10\"/>\n```"
        );
    }

    #[tokio::test]
    async fn custom_output_schema_does_not_change_default_turns() {
        let custom = json!({"type":"object","properties":{"logic":{"type":"string"}},"required":["logic"],"additionalProperties":false});
        for schema in [Some(custom), None] {
            let (mut session, mut socket) = test_connection().await;
            let expected = schema.clone().unwrap_or_else(output_schema);
            let task = tokio::spawn(async move {
                match schema {
                    Some(schema) => session.turn_with_schema("generate", schema).await,
                    None => session.turn("generate").await,
                }
            });
            let request = client_message(&mut socket).await;
            assert_eq!(request["params"]["outputSchema"], expected);
            server_message(
                &mut socket,
                json!({"id":request["id"],"result":{"turn":{"id":"turn-1"}}}),
            )
            .await;
            server_message(&mut socket, json!({"method":"item/completed","params":{"threadId":"test-thread","turnId":"turn-1","item":{"type":"agentMessage","text":"{}"}}})).await;
            complete_test_turn(&mut socket, "completed").await;
            assert_eq!(task.await.unwrap().unwrap(), "{}");
        }
    }

    #[tokio::test]
    async fn quantitative_generation_uses_its_own_schema_and_decodes_ten_questions() {
        use super::super::data_analysis::CodexConfig;
        use crate::application::quantitative_relations::{
            QuantitativeGenerationPort, QuantitativeGenerationRequest, QuantitativeGenerationTarget,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let generator = CodexConfig::for_test_server(listener.local_addr().unwrap());
        let request = QuantitativeGenerationRequest {
            targets: (0..10)
                .map(|i| QuantitativeGenerationTarget {
                    target_id: format!("target-{i}"),
                    card_id: format!("card-{i}"),
                    card_path: "数量/经济利润/cards.md".into(),
                    front: "利润率".into(),
                    back: "利润除以成本".into(),
                    correct_answer: Vec::new(),
                    kind: "quantitative".into(),
                })
                .collect(),
            user_prompt: "数量出题偏好".into(),
        };
        let task =
            tokio::spawn(async move { generator.generate(request, watch::channel(false).0).await });
        let mut socket = accept_session(&listener).await;
        let turn = client_message(&mut socket).await;
        let schema = &turn["params"]["outputSchema"];
        assert_eq!(schema["required"], json!(["questions"]));
        assert_eq!(schema["properties"]["questions"]["minItems"], 10);
        assert_eq!(schema["properties"]["questions"]["maxItems"], 10);
        assert_eq!(
            schema["properties"]["questions"]["items"]["properties"]["kind"]["enum"],
            json!(["quantitative"])
        );
        let questions: Vec<_> = (0..10)
            .map(|i| {
                json!({
                    "target_id": format!("target-{i}"), "kind":"quantitative",
                    "question":format!("数量题 {i}"), "options":["1","2","3","4"],
                    "correct_index":0, "explanation":"数量解析"
                })
            })
            .collect();
        server_message(
            &mut socket,
            json!({"id":turn["id"],"result":{"turn":{"id":"turn-1"}}}),
        )
        .await;
        server_message(
            &mut socket,
            json!({"method":"item/completed","params":{
                "threadId":"test-thread","turnId":"turn-1",
                "item":{"type":"agentMessage","text":json!({"questions":questions}).to_string()}
            }}),
        )
        .await;
        complete_test_turn(&mut socket, "completed").await;
        let generated = task.await.unwrap().unwrap();
        assert_eq!(generated.len(), 10);
        assert!(generated.iter().all(|q| q.kind == "quantitative"));
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
    async fn more_than_2048_notifications_do_not_abort_a_healthy_turn() {
        let (mut session, mut socket) = test_connection().await;
        let worker = tokio::spawn(async move { session.turn("generate").await });
        let turn = client_message(&mut socket).await;
        server_message(
            &mut socket,
            json!({"id":turn["id"],"result":{"turn":{"id":"turn-1"}}}),
        )
        .await;
        for _ in 0..2100 {
            server_message(
                &mut socket,
                json!({"method":"item/started","params":{
                    "threadId":"test-thread","turnId":"turn-1","item":{"type":"reasoning"}
                }}),
            )
            .await;
        }
        server_message(
            &mut socket,
            json!({"method":"item/completed","params":{
                "threadId":"test-thread","turnId":"turn-1",
                "item":{"type":"agentMessage","text":"{\"questions\":[]}"}
            }}),
        )
        .await;
        complete_test_turn(&mut socket, "completed").await;
        assert_eq!(worker.await.unwrap().unwrap(), "{\"questions\":[]}");
    }

    #[tokio::test]
    async fn notification_between_old_and_new_limit_is_not_rejected() {
        let (mut session, mut socket) = test_connection().await;
        let worker = tokio::spawn(async move { session.turn("generate").await });
        start_test_turn(&mut socket).await;
        server_large_message(
            &mut socket,
            &json!({"method":"item/completed","params":{
                "threadId":"test-thread","turnId":"turn-1",
                "item":{"type":"commandExecution","aggregatedOutput":"x".repeat(400 * 1024)}
            }}),
        )
        .await;
        server_message(
            &mut socket,
            json!({"method":"item/completed","params":{
                "threadId":"test-thread","turnId":"turn-1",
                "item":{"type":"agentMessage","text":"{\"questions\":[]}"}
            }}),
        )
        .await;
        complete_test_turn(&mut socket, "completed").await;
        assert_eq!(worker.await.unwrap().unwrap(), "{\"questions\":[]}");
    }

    #[tokio::test]
    async fn review_accepts_large_tool_output_and_completes() {
        let (mut session, mut socket) = test_connection().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sink: AiReviewEventSink = Arc::new(move |event| {
            let tx = tx.clone();
            Box::pin(async move {
                tx.send(event)
                    .map_err(|_| "event receiver closed".to_owned())
            })
        });
        let worker = tokio::spawn(async move {
            session
                .turn_review_stream("review", &[], watch::channel(false).0, sink)
                .await
        });
        start_test_turn(&mut socket).await;
        server_message(&mut socket, json!({"method":"item/started","params":{
            "threadId":"test-thread","turnId":"turn-1","item":{"id":"cmd","type":"commandExecution"}
        }})).await;
        server_large_message(&mut socket, &json!({"method":"item/commandExecution/outputDelta","params":{
            "threadId":"test-thread","turnId":"turn-1","itemId":"cmd","delta":"x".repeat(600 * 1024)
        }})).await;
        server_large_message(&mut socket, &json!({"method":"item/completed","params":{
            "threadId":"test-thread","turnId":"turn-1","item":{"id":"cmd","type":"commandExecution","aggregatedOutput":"x".repeat(600 * 1024)}
        }})).await;
        server_message(&mut socket, json!({"method":"item/completed","params":{
            "threadId":"test-thread","turnId":"turn-1","item":{"type":"agentMessage","text":"完成"}
        }})).await;
        complete_test_turn(&mut socket, "completed").await;
        assert_eq!(worker.await.unwrap().unwrap(), "完成");
        let mut output = String::new();
        while let Ok(event) = rx.try_recv() {
            if let AiReviewProgress::CommandOutputDelta { text, .. } = event {
                output.push_str(&text);
            }
        }
        assert_eq!(output.len(), 600 * 1024);
    }

    #[tokio::test]
    async fn oversized_notification_is_discarded_and_turn_completes() {
        let (mut session, mut socket) = test_connection().await;
        let worker = tokio::spawn(async move { session.turn("generate").await });
        start_test_turn(&mut socket).await;
        server_large_message(
            &mut socket,
            &json!({"method":"item/completed","params":{
                "threadId":"test-thread","turnId":"turn-1",
                "item":{"type":"commandExecution","aggregatedOutput":"x".repeat(600 * 1024)}
            }}),
        )
        .await;
        server_message(
            &mut socket,
            json!({"method":"item/completed","params":{
                "threadId":"test-thread","turnId":"turn-1",
                "item":{"type":"agentMessage","text":"{\"questions\":[]}"}
            }}),
        )
        .await;
        complete_test_turn(&mut socket, "completed").await;
        assert_eq!(worker.await.unwrap().unwrap(), "{\"questions\":[]}");
    }

    #[tokio::test]
    async fn oversized_fragmented_notification_is_discarded() {
        let (mut session, mut socket) = test_connection().await;
        let worker = tokio::spawn(async move { session.turn("generate").await });
        start_test_turn(&mut socket).await;
        let message = json!({"method":"item/completed","params":{
            "threadId":"test-thread","turnId":"turn-1",
            "item":{"type":"commandExecution","aggregatedOutput":"x".repeat(600 * 1024)}
        }});
        let bytes = serde_json::to_vec(&message).unwrap();
        let split = bytes.len() / 2;
        server_frame(&mut socket, false, 0x1, &bytes[..split]).await;
        server_frame(&mut socket, true, 0x0, &bytes[split..]).await;
        server_message(
            &mut socket,
            json!({"method":"item/completed","params":{
                "threadId":"test-thread","turnId":"turn-1",
                "item":{"type":"agentMessage","text":"{\"questions\":[]}"}
            }}),
        )
        .await;
        complete_test_turn(&mut socket, "completed").await;
        assert_eq!(worker.await.unwrap().unwrap(), "{\"questions\":[]}");
    }

    #[test]
    fn packet_limit_is_double_the_previous_limit() {
        assert_eq!(MAX_PACKET, 512 * 1024);
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
