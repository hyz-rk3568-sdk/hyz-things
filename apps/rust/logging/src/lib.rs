use std::{
    ffi::CString,
    io::{self, Write},
    os::raw::c_int,
    sync::Arc,
};

use tracing_subscriber::{filter::LevelFilter, fmt::MakeWriter};

pub const MAX_SYSLOG_MESSAGE_BYTES: usize = 4 * 1024;
const SYSLOG_IDENT_PREFIX: &[u8] = b"hyz\0";
const SYSLOG_FORMAT: &[u8] = b"%s\0";

pub trait SyslogSink: Send + Sync {
    fn send(&self, priority: c_int, message: &[u8]);
}

struct LibcSyslogSink {
    _ident: CString,
}

impl LibcSyslogSink {
    fn new(service: &str) -> io::Result<Self> {
        let ident = CString::new(service).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "syslog service name contains an interior NUL",
            )
        })?;
        let ident_ptr = if service.is_empty() {
            SYSLOG_IDENT_PREFIX.as_ptr().cast()
        } else {
            ident.as_ptr()
        };
        unsafe {
            libc::openlog(
                ident_ptr,
                libc::LOG_PID | libc::LOG_NDELAY,
                libc::LOG_DAEMON,
            );
        }
        Ok(Self { _ident: ident })
    }
}

impl SyslogSink for LibcSyslogSink {
    fn send(&self, priority: c_int, message: &[u8]) {
        let Ok(message) = CString::new(message) else {
            return;
        };
        unsafe {
            libc::syslog(priority, SYSLOG_FORMAT.as_ptr().cast(), message.as_ptr());
        }
    }
}

#[derive(Clone)]
pub struct SyslogMakeWriter {
    sink: Arc<dyn SyslogSink>,
}

impl SyslogMakeWriter {
    pub fn new(service: &str) -> io::Result<Self> {
        Ok(Self {
            sink: Arc::new(LibcSyslogSink::new(service)?),
        })
    }
}

impl<'a> MakeWriter<'a> for SyslogMakeWriter {
    type Writer = SyslogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SyslogWriter::with_sink(self.sink.clone(), libc::LOG_INFO)
    }

    fn make_writer_for(&'a self, metadata: &tracing::Metadata<'_>) -> Self::Writer {
        SyslogWriter::with_sink(self.sink.clone(), priority_for(metadata.level()))
    }
}

pub struct SyslogWriter {
    sink: Arc<dyn SyslogSink>,
    priority: c_int,
    buffer: Vec<u8>,
    discard_until_newline: bool,
}

impl SyslogWriter {
    pub fn with_sink(sink: Arc<dyn SyslogSink>, priority: c_int) -> Self {
        Self {
            sink,
            priority,
            buffer: Vec::new(),
            discard_until_newline: false,
        }
    }

    fn emit_line(&self, line: &[u8]) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        self.sink.send(self.priority, &truncate_message(line));
    }

    fn append_bounded(&mut self, bytes: &[u8]) {
        let available = MAX_SYSLOG_MESSAGE_BYTES.saturating_sub(self.buffer.len());
        let copied = bytes.len().min(available);
        self.buffer.extend_from_slice(&bytes[..copied]);
        if copied < bytes.len() {
            self.discard_until_newline = true;
        }
    }
}

impl Write for SyslogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut remaining = bytes;
        while !remaining.is_empty() {
            if self.discard_until_newline {
                let Some(position) = remaining.iter().position(|byte| *byte == b'\n') else {
                    break;
                };
                let line = std::mem::take(&mut self.buffer);
                self.emit_line(&line);
                self.discard_until_newline = false;
                remaining = &remaining[position + 1..];
                continue;
            }

            if let Some(position) = remaining.iter().position(|byte| *byte == b'\n') {
                self.append_bounded(&remaining[..position]);
                let line = std::mem::take(&mut self.buffer);
                self.emit_line(&line);
                self.discard_until_newline = false;
                remaining = &remaining[position + 1..];
            } else {
                self.append_bounded(remaining);
                break;
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            self.emit_line(&line);
        }
        self.discard_until_newline = false;
        Ok(())
    }
}

pub fn priority_for(level: &tracing::Level) -> c_int {
    match *level {
        tracing::Level::ERROR => libc::LOG_ERR,
        tracing::Level::WARN => libc::LOG_WARNING,
        tracing::Level::INFO => libc::LOG_INFO,
        tracing::Level::DEBUG | tracing::Level::TRACE => libc::LOG_DEBUG,
    }
}

pub fn truncate_message(message: &[u8]) -> Vec<u8> {
    if message.len() <= MAX_SYSLOG_MESSAGE_BYTES {
        return message.to_vec();
    }
    let mut end = MAX_SYSLOG_MESSAGE_BYTES;
    while end > 0 && std::str::from_utf8(&message[..end]).is_err() {
        end -= 1;
    }
    message[..end].to_vec()
}

pub fn init(service: &str) -> Result<(), Box<dyn std::error::Error>> {
    let writer = SyslogMakeWriter::new(service)?;
    tracing_subscriber::fmt()
        .json()
        .with_target(true)
        .with_ansi(false)
        .with_max_level(LevelFilter::INFO)
        .with_writer(writer)
        .try_init()
        .map_err(|error| error as Box<dyn std::error::Error>)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSink {
        messages: Mutex<Vec<(c_int, Vec<u8>)>>,
    }

    impl SyslogSink for RecordingSink {
        fn send(&self, priority: c_int, message: &[u8]) {
            self.messages
                .lock()
                .unwrap()
                .push((priority, message.to_vec()));
        }
    }

    #[test]
    fn priority_matches_tracing_level() {
        assert_eq!(priority_for(&tracing::Level::INFO), libc::LOG_INFO);
        assert_eq!(priority_for(&tracing::Level::WARN), libc::LOG_WARNING);
        assert_eq!(priority_for(&tracing::Level::ERROR), libc::LOG_ERR);
        assert_eq!(priority_for(&tracing::Level::DEBUG), libc::LOG_DEBUG);
    }

    #[test]
    fn writer_emits_one_message_per_line() {
        let sink = Arc::new(RecordingSink::default());
        let mut writer = SyslogWriter::with_sink(sink.clone(), libc::LOG_WARNING);
        writer
            .write_all(b"{\"event\":\"tls_handshake_timeout\"}\n")
            .unwrap();
        writer.flush().unwrap();

        let messages = sink.messages.lock().unwrap();
        assert_eq!(
            messages.as_slice(),
            &[(
                libc::LOG_WARNING,
                br#"{"event":"tls_handshake_timeout"}"#.to_vec(),
            )]
        );
    }

    #[test]
    fn writer_buffer_is_bounded_before_flush() {
        let sink = Arc::new(RecordingSink::default());
        let mut writer = SyslogWriter::with_sink(sink.clone(), libc::LOG_INFO);
        writer
            .write_all(&vec![b'x'; MAX_SYSLOG_MESSAGE_BYTES + 100])
            .unwrap();

        assert_eq!(writer.buffer.len(), MAX_SYSLOG_MESSAGE_BYTES);
        writer.flush().unwrap();
        let messages = sink.messages.lock().unwrap();
        assert_eq!(messages[0].1.len(), MAX_SYSLOG_MESSAGE_BYTES);
    }

    #[test]
    fn oversized_line_does_not_swallow_the_following_line() {
        let sink = Arc::new(RecordingSink::default());
        let mut writer = SyslogWriter::with_sink(sink.clone(), libc::LOG_INFO);
        writer
            .write_all(&vec![b'x'; MAX_SYSLOG_MESSAGE_BYTES + 100])
            .unwrap();
        writer.write_all(b"\nnext\n").unwrap();
        writer.flush().unwrap();

        let messages = sink.messages.lock().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].1.len(), MAX_SYSLOG_MESSAGE_BYTES);
        assert_eq!(messages[1].1, b"next");
    }

    #[test]
    fn message_size_is_bounded() {
        let bounded = truncate_message(&vec![b'x'; MAX_SYSLOG_MESSAGE_BYTES + 100]);
        assert_eq!(bounded.len(), MAX_SYSLOG_MESSAGE_BYTES);
    }
}
