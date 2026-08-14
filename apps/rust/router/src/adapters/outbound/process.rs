use super::{
    paths::{
        MIHOMO_CHECK_LOG, MIHOMO_CONTROLLER_SECRET, MIHOMO_DATA_DIR, MIHOMO_EXECUTABLE, MIHOMO_LOG,
        MIHOMO_RUNTIME_CONFIG, MIHOMO_STATE_DIR, MIHOMO_TUN_IDENTITY, MIHOMO_WATCHER_LOG,
        TAILSCALE_EXECUTABLE,
    },
    proxy::mihomo_tun_ifindex,
};
use crate::{
    application::{
        fail_open::WatcherInvocation,
        ports::{CoreIdentity, CoreRecordState, PlatformError},
    },
    domain::proxy::MIHOMO_TUN_INTERFACE,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::{SocketAddr, SocketAddrV4},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const NETWORK_LOCK: &str = "/run/hyz-network.lock";
pub const MIHOMO_PID_RECORD: &str = "/run/hyz-mihomo/core.pid";
pub const MIHOMO_WATCHER_RECORD: &str = "/run/hyz-mihomo/watch.pid";
const SHUTDOWN_LOG: &str = "/run/hyz-router/shutdown.log";
const MAX_SHUTDOWN_LOG: usize = 8 * 1024;
const MAX_COMMAND_OUTPUT: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RECORD_SIZE: usize = 4096;
const IDENTITY_EXIT_ATTEMPTS: usize = 60;
const IDENTITY_EXIT_POLL: Duration = Duration::from_millis(50);

extern "C" {
    fn setsid() -> i32;
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Tool {
    Ip,
    Iptables,
    WpaCli,
    HostapdCli,
    Mihomo,
    Tailscale,
    Kill,
}

impl Tool {
    fn path(self) -> &'static str {
        match self {
            Self::Ip => "/usr/sbin/ip",
            Self::Iptables => "/usr/sbin/iptables",
            Self::WpaCli => "/usr/sbin/wpa_cli",
            Self::HostapdCli => "/usr/bin/hostapd_cli",
            Self::Mihomo => MIHOMO_EXECUTABLE,
            Self::Tailscale => TAILSCALE_EXECUTABLE,
            Self::Kill => "/bin/kill",
        }
    }
}

#[derive(Debug)]
pub(crate) struct FixedOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

fn read_bounded_command_stream(mut stream: impl Read, max_output: usize) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > max_output {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fixed command output exceeded limit",
            ));
        }
        output.extend_from_slice(&chunk[..read]);
    }
}

#[derive(Debug, Default, Clone)]
pub struct LinuxRouterPlatform;

#[derive(Debug, Default, Clone)]
pub struct LinuxMihomoFailOpenPlatform {
    pub(crate) platform: LinuxRouterPlatform,
}

impl LinuxMihomoFailOpenPlatform {
    pub const fn new() -> Self {
        Self {
            platform: LinuxRouterPlatform::new(),
        }
    }
}

impl LinuxRouterPlatform {
    pub const fn new() -> Self {
        Self
    }

    pub fn clear_shutdown_failure_log(&self) -> Result<(), PlatformError> {
        super::storage::remove_file_durable(SHUTDOWN_LOG)
    }

    pub fn record_shutdown_failure(&self, message: &str) -> Result<(), PlatformError> {
        let mut log = private_log(SHUTDOWN_LOG)?;
        let message = message.chars().take(MAX_SHUTDOWN_LOG).collect::<String>();
        log.write_all(message.as_bytes())
            .and_then(|()| log.write_all(b"\n"))
            .map_err(|error| PlatformError::Io(format!("write shutdown log: {error}")))?;
        log.sync_all()
            .map_err(|error| PlatformError::Io(format!("sync shutdown log: {error}")))
    }

    pub(crate) fn run(&self, tool: Tool, args: &[String]) -> Result<FixedOutput, PlatformError> {
        self.run_with_timeout(tool, args, COMMAND_TIMEOUT)
    }

    pub(crate) fn run_with_timeout(
        &self,
        tool: Tool,
        args: &[String],
        timeout: Duration,
    ) -> Result<FixedOutput, PlatformError> {
        let output = self.run_probe_with_timeout(tool, args, timeout)?;
        if !output.success {
            return Err(PlatformError::CommandFailed(format!(
                "{} failed: {}",
                tool.path(),
                output.stderr.trim()
            )));
        }
        Ok(output)
    }

    pub(crate) fn run_probe(
        &self,
        tool: Tool,
        args: &[String],
    ) -> Result<FixedOutput, PlatformError> {
        self.run_probe_with_timeout(tool, args, COMMAND_TIMEOUT)
    }

    pub(crate) fn run_probe_with_timeout(
        &self,
        tool: Tool,
        args: &[String],
        timeout: Duration,
    ) -> Result<FixedOutput, PlatformError> {
        self.run_probe_with_timeout_and_limit(tool, args, timeout, MAX_COMMAND_OUTPUT)
    }

    pub(crate) fn run_probe_with_timeout_and_limit(
        &self,
        tool: Tool,
        args: &[String],
        timeout: Duration,
        max_output: usize,
    ) -> Result<FixedOutput, PlatformError> {
        let mut child = Command::new(tool.path())
            .args(args)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                PlatformError::Io(format!("could not start fixed command: {error}"))
            })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            PlatformError::Io("fixed command stdout pipe is unavailable".to_owned())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            PlatformError::Io("fixed command stderr pipe is unavailable".to_owned())
        })?;
        let stdout_reader = thread::spawn(move || read_bounded_command_stream(stdout, max_output));
        let stderr_reader = thread::spawn(move || read_bounded_command_stream(stderr, max_output));
        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|error| {
                PlatformError::Io(format!("could not wait for command: {error}"))
            })? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(PlatformError::CommandFailed(
                    "fixed command exceeded its deadline".to_owned(),
                ));
            }
            thread::sleep(Duration::from_millis(20));
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| PlatformError::CommandFailed("fixed stdout reader panicked".to_owned()))?
            .map_err(|error| PlatformError::CommandFailed(error.to_string()))?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| PlatformError::CommandFailed("fixed stderr reader panicked".to_owned()))?
            .map_err(|error| PlatformError::CommandFailed(error.to_string()))?;
        if stdout.len() + stderr.len() > max_output {
            return Err(PlatformError::CommandFailed(
                "fixed command output exceeded limit".to_owned(),
            ));
        }
        let success = status.success();
        let stdout = String::from_utf8(stdout).map_err(|_| {
            PlatformError::CommandFailed("fixed command emitted non-UTF-8 stdout".to_owned())
        })?;
        let stderr = String::from_utf8(stderr).map_err(|_| {
            PlatformError::CommandFailed("fixed command emitted non-UTF-8 stderr".to_owned())
        })?;
        Ok(FixedOutput {
            success,
            stdout,
            stderr,
        })
    }

    pub(crate) fn validate_mihomo_config(&self) -> Result<(), PlatformError> {
        self.validate_mihomo_config_at(MIHOMO_RUNTIME_CONFIG)
    }

    pub(crate) fn validate_mihomo_config_at(&self, config: &str) -> Result<(), PlatformError> {
        super::storage::ensure_private_dir(MIHOMO_STATE_DIR)?;
        let log = private_log(MIHOMO_CHECK_LOG)?;
        let stderr = log
            .try_clone()
            .map_err(|error| PlatformError::Io(format!("clone config-check log: {error}")))?;
        let mut child = Command::new(Tool::Mihomo.path())
            .args(["-t", "-d", MIHOMO_DATA_DIR, "-f", config])
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| PlatformError::Io(format!("start Mihomo config test: {error}")))?;
        wait_command(&mut child, COMMAND_TIMEOUT, "Mihomo config test")
    }

    pub(crate) fn start_mihomo(&self) -> Result<(), PlatformError> {
        if self.mihomo_identity()?.is_some() {
            return Ok(());
        }
        if mihomo_tun_ifindex()?.is_some() || self.read_mihomo_tun_identity()?.is_some() {
            return Err(PlatformError::Conflict(
                "stale or foreign hyz-mihomo interface identity blocks Mihomo startup".to_owned(),
            ));
        }
        super::storage::ensure_private_dir(MIHOMO_DATA_DIR)?;
        super::storage::ensure_private_dir(MIHOMO_STATE_DIR)?;
        let runtime_config =
            super::storage::read_private_small_optional(MIHOMO_RUNTIME_CONFIG, 4 * 1024 * 1024)?
                .ok_or_else(|| {
                    PlatformError::InvalidState("Mihomo runtime config is absent".to_owned())
                })?;
        let config_sha256 = sha256_hex(runtime_config.as_bytes());
        let log = private_log(MIHOMO_LOG)?;
        let stderr = log
            .try_clone()
            .map_err(|error| PlatformError::Io(format!("clone Mihomo log: {error}")))?;
        let mut child = Command::new(Tool::Mihomo.path())
            .args(core_args())
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| PlatformError::Io(format!("start Mihomo directly: {error}")))?;
        let pid = child.id();
        let start_time = match process_start_time(pid) {
            Ok(Some(start)) => start,
            Ok(None) => {
                let _ = child.wait();
                return Err(PlatformError::InvalidState(
                    "new Mihomo process disappeared".to_owned(),
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let identity = ProcessIdentity {
            core: CoreIdentity { pid, start_time },
            executable: PathBuf::from(Tool::Mihomo.path()),
            argv: core_argv(),
            config_sha256,
        };
        if let Err(error) =
            super::storage::atomic_write_private(MIHOMO_PID_RECORD, identity.serialize().as_bytes())
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        if !identity.matches_live_process()? {
            let _ = child.kill();
            let _ = child.wait();
            let _ = super::storage::remove_file_durable(MIHOMO_PID_RECORD);
            return Err(PlatformError::InvalidState(
                "new Mihomo identity did not match exact PID/start/exe/argv/config SHA-256"
                    .to_owned(),
            ));
        }
        reap_in_background(child, "mihomo");
        if let Err(error) = self.wait_for_mihomo_controller() {
            let _ = self.stop_mihomo();
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn stop_mihomo(&self) -> Result<(), PlatformError> {
        let Some(identity) = self.mihomo_identity()? else {
            if mihomo_tun_ifindex()?.is_some() || self.read_mihomo_tun_identity()?.is_some() {
                return Err(PlatformError::Conflict(
                    "refusing Mihomo cleanup with stale or foreign TUN identity".to_owned(),
                ));
            }
            return remove_mihomo_runtime_credentials();
        };
        let tun_owned = match (self.read_mihomo_tun_identity()?, mihomo_tun_ifindex()?) {
            (None, None) => false,
            (Some(tun), Some(ifindex))
                if tun.core == identity.core
                    && tun.ifindex == ifindex
                    && mihomo_process_holds_tun(identity.core)? =>
            {
                true
            }
            _ => {
                return Err(PlatformError::Conflict(
                    "refusing to stop Mihomo while hyz-mihomo ownership is mismatched".to_owned(),
                ))
            }
        };
        self.run(
            Tool::Kill,
            &["-TERM".to_owned(), identity.core.pid.to_string()],
        )?;
        if wait_for_identity_disappearance(IDENTITY_EXIT_ATTEMPTS, IDENTITY_EXIT_POLL, || {
            identity.matches_process_only()
        })? {
            finish_mihomo_stop(tun_owned)?;
            return Ok(());
        }
        self.run(
            Tool::Kill,
            &["-KILL".to_owned(), identity.core.pid.to_string()],
        )?;
        if wait_for_identity_disappearance(IDENTITY_EXIT_ATTEMPTS, IDENTITY_EXIT_POLL, || {
            identity.matches_process_only()
        })? {
            finish_mihomo_stop(tun_owned)
        } else {
            Err(PlatformError::Conflict(
                "exact Mihomo identity remained live after TERM and KILL; retaining its record"
                    .to_owned(),
            ))
        }
    }

    pub(crate) fn start_mihomo_watcher(&self) -> Result<(), PlatformError> {
        if let Some(watcher) = self.read_watcher_record()? {
            let core = self.mihomo_identity()?;
            if watcher.matches_live_process()?
                && core
                    .as_ref()
                    .is_some_and(|identity| identity.core == watcher.core)
            {
                return Ok(());
            }
            return Err(PlatformError::Conflict(
                "refusing to replace a Mihomo watcher record without exact live watcher/core identity"
                    .to_owned(),
            ));
        }
        let core = self.mihomo_identity()?.ok_or_else(|| {
            PlatformError::InvalidState(
                "cannot start watcher without exact core identity".to_owned(),
            )
        })?;
        let executable = fs::read_link("/proc/self/exe").map_err(|error| {
            PlatformError::ProbeFailed(format!("read current executable: {error}"))
        })?;
        let invocation = WatcherInvocation { core: core.core };
        let args = invocation.exact_args();
        let mut command = Command::new(&executable);
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(|| {
                if setsid() == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let mut child = command.spawn().map_err(|error| {
            PlatformError::Io(format!("spawn detached Mihomo watcher: {error}"))
        })?;
        let pid = child.id();
        let start_time = process_start_time(pid)?.ok_or_else(|| {
            PlatformError::InvalidState("new watcher disappeared before identity record".to_owned())
        })?;
        let watcher = WatcherIdentity {
            process: CoreIdentity { pid, start_time },
            executable,
            argv: watcher_argv(invocation),
            core: invocation.core,
        };
        if let Err(error) = super::storage::atomic_write_private(
            MIHOMO_WATCHER_RECORD,
            watcher.serialize().as_bytes(),
        ) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        reap_in_background(child, "mihomo-watch");
        Ok(())
    }

    pub(crate) fn wait_for_mihomo_watcher(&self) -> Result<(), PlatformError> {
        for _ in 0..50 {
            let core = self.mihomo_identity()?;
            let watcher = self.mihomo_watcher_identity()?;
            if matches!((core.as_ref(), watcher.as_ref()), (Some(core), Some(watcher)) if core.core == watcher.core)
            {
                return Ok(());
            }
            if core.is_none() {
                return Err(PlatformError::InvalidState(
                    "Mihomo core exited before watcher became ready".to_owned(),
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(PlatformError::UnsafeToCutOver(
            "Mihomo watcher exact identity did not become ready".to_owned(),
        ))
    }

    pub(crate) fn stop_mihomo_watcher(&self) -> Result<(), PlatformError> {
        let Some(identity) = self.read_watcher_record()? else {
            return Ok(());
        };
        if !identity.matches_live_process()? {
            return super::storage::remove_file_durable(MIHOMO_WATCHER_RECORD);
        }
        if identity.process.pid == std::process::id() {
            return Err(PlatformError::Conflict(
                "watcher refuses to stop itself through normal lifecycle action".to_owned(),
            ));
        }
        self.run(
            Tool::Kill,
            &["-TERM".to_owned(), identity.process.pid.to_string()],
        )?;
        if wait_for_identity_disappearance(IDENTITY_EXIT_ATTEMPTS, IDENTITY_EXIT_POLL, || {
            identity.matches_live_process()
        })? {
            super::storage::remove_file_durable(MIHOMO_WATCHER_RECORD)?;
            return Ok(());
        }
        self.run(
            Tool::Kill,
            &["-KILL".to_owned(), identity.process.pid.to_string()],
        )?;
        if wait_for_identity_disappearance(IDENTITY_EXIT_ATTEMPTS, IDENTITY_EXIT_POLL, || {
            identity.matches_live_process()
        })? {
            super::storage::remove_file_durable(MIHOMO_WATCHER_RECORD)
        } else {
            Err(PlatformError::Conflict(
                "exact Mihomo watcher identity remained live after TERM and KILL; retaining its record"
                    .to_owned(),
            ))
        }
    }

    pub(crate) fn mihomo_process_count(&self) -> Result<usize, PlatformError> {
        count_executable_processes(Path::new(Tool::Mihomo.path()))
    }

    pub(crate) fn mihomo_runtime_config_matches(&self) -> Result<bool, PlatformError> {
        let Some(identity) = self.mihomo_identity()? else {
            return Ok(false);
        };
        let Some(config) =
            super::storage::read_private_small_optional(MIHOMO_RUNTIME_CONFIG, 4 * 1024 * 1024)?
        else {
            return Ok(false);
        };
        Ok(sha256_hex(config.as_bytes()) == identity.config_sha256)
    }

    pub(crate) fn mihomo_identity(&self) -> Result<Option<ProcessIdentity>, PlatformError> {
        let Some(identity) = self.read_mihomo_record()? else {
            return Ok(None);
        };
        Ok(identity.matches_live_process()?.then_some(identity))
    }

    pub(crate) fn mihomo_watcher_identity(&self) -> Result<Option<WatcherIdentity>, PlatformError> {
        let Some(identity) = self.read_watcher_record()? else {
            return Ok(None);
        };
        Ok(identity.matches_live_process()?.then_some(identity))
    }

    pub(crate) fn read_mihomo_record(&self) -> Result<Option<ProcessIdentity>, PlatformError> {
        let Some(record) =
            super::storage::read_private_small_optional(MIHOMO_PID_RECORD, MAX_RECORD_SIZE)?
        else {
            return Ok(None);
        };
        ProcessIdentity::parse(&record).map(Some)
    }

    pub(crate) fn read_watcher_record(&self) -> Result<Option<WatcherIdentity>, PlatformError> {
        let Some(record) =
            super::storage::read_private_small_optional(MIHOMO_WATCHER_RECORD, MAX_RECORD_SIZE)?
        else {
            return Ok(None);
        };
        WatcherIdentity::parse(&record).map(Some)
    }

    pub(crate) fn core_record_state_exact(
        &self,
        expected: CoreIdentity,
    ) -> Result<CoreRecordState, PlatformError> {
        let Some(record) = self.read_mihomo_record()? else {
            return Ok(CoreRecordState::Absent);
        };
        if record.core != expected {
            return Ok(CoreRecordState::Replaced);
        }
        if record.matches_live_process()? {
            Ok(CoreRecordState::ExpectedLive)
        } else {
            Ok(CoreRecordState::ExpectedExited)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessIdentity {
    pub core: CoreIdentity,
    executable: PathBuf,
    argv: Vec<Vec<u8>>,
    config_sha256: String,
}

impl ProcessIdentity {
    fn serialize(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n",
            self.core.pid,
            self.core.start_time,
            self.executable.display(),
            self.config_sha256,
            encode_argv(&self.argv)
        )
    }

    fn parse(record: &str) -> Result<Self, PlatformError> {
        let mut lines = record.lines();
        let pid = parse_u32(lines.next(), "Mihomo PID")?;
        let start_time = parse_u64(lines.next(), "Mihomo start time")?;
        let executable = PathBuf::from(lines.next().ok_or_else(|| {
            PlatformError::InvalidState("Mihomo executable record is absent".to_owned())
        })?);
        let config_sha256 = parse_sha256(lines.next())?;
        let argv = decode_argv(lines.next().ok_or_else(|| {
            PlatformError::InvalidState("Mihomo argv record is absent".to_owned())
        })?)?;
        if lines.next().is_some()
            || executable.as_path() != Path::new(Tool::Mihomo.path())
            || argv != core_argv()
        {
            return Err(PlatformError::InvalidState(
                "Mihomo identity record has unexpected executable/argv/fields".to_owned(),
            ));
        }
        Ok(Self {
            core: CoreIdentity { pid, start_time },
            executable,
            argv,
            config_sha256,
        })
    }

    fn matches_process_only(&self) -> Result<bool, PlatformError> {
        process_identity_matches(self.core, &self.executable, &self.argv)
    }

    fn matches_live_process(&self) -> Result<bool, PlatformError> {
        if !self.matches_process_only()? {
            return Ok(false);
        }
        let Some(config) =
            super::storage::read_private_small_optional(MIHOMO_RUNTIME_CONFIG, 4 * 1024 * 1024)?
        else {
            return Ok(false);
        };
        Ok(sha256_hex(config.as_bytes()) == self.config_sha256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WatcherIdentity {
    pub process: CoreIdentity,
    executable: PathBuf,
    argv: Vec<Vec<u8>>,
    pub core: CoreIdentity,
}

impl WatcherIdentity {
    fn serialize(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n",
            self.process.pid,
            self.process.start_time,
            self.executable.display(),
            self.core.pid,
            self.core.start_time,
            encode_argv(&self.argv)
        )
    }

    fn parse(record: &str) -> Result<Self, PlatformError> {
        let mut lines = record.lines();
        let pid = parse_u32(lines.next(), "watcher PID")?;
        let start_time = parse_u64(lines.next(), "watcher start time")?;
        let executable = PathBuf::from(lines.next().ok_or_else(|| {
            PlatformError::InvalidState("watcher executable record is absent".to_owned())
        })?);
        let core_pid = parse_u32(lines.next(), "watched core PID")?;
        let core_start = parse_u64(lines.next(), "watched core start time")?;
        let argv = decode_argv(lines.next().ok_or_else(|| {
            PlatformError::InvalidState("watcher argv record is absent".to_owned())
        })?)?;
        let core = CoreIdentity {
            pid: core_pid,
            start_time: core_start,
        };
        let current_executable = fs::read_link("/proc/self/exe").map_err(|error| {
            PlatformError::ProbeFailed(format!("read current executable: {error}"))
        })?;
        if lines.next().is_some()
            || executable != current_executable
            || argv != watcher_argv(WatcherInvocation { core })
        {
            return Err(PlatformError::InvalidState(
                "watcher identity record has unexpected argv/fields".to_owned(),
            ));
        }
        Ok(Self {
            process: CoreIdentity { pid, start_time },
            executable,
            argv,
            core,
        })
    }

    pub(crate) fn matches_live_process(&self) -> Result<bool, PlatformError> {
        process_identity_matches(self.process, &self.executable, &self.argv)
    }
}

fn core_args() -> [&'static str; 4] {
    ["-d", MIHOMO_DATA_DIR, "-f", MIHOMO_RUNTIME_CONFIG]
}

fn core_argv() -> Vec<Vec<u8>> {
    std::iter::once(Tool::Mihomo.path().as_bytes().to_vec())
        .chain(core_args().into_iter().map(|arg| arg.as_bytes().to_vec()))
        .collect()
}

fn watcher_argv(invocation: WatcherInvocation) -> Vec<Vec<u8>> {
    let executable =
        fs::read_link("/proc/self/exe").unwrap_or_else(|_| PathBuf::from("/proc/self/exe"));
    std::iter::once(executable.as_os_str().as_bytes().to_vec())
        .chain(
            invocation
                .exact_args()
                .into_iter()
                .map(|arg| arg.into_bytes()),
        )
        .collect()
}

pub(crate) fn process_identity_matches(
    identity: CoreIdentity,
    executable: &Path,
    argv: &[Vec<u8>],
) -> Result<bool, PlatformError> {
    if process_start_time(identity.pid)? != Some(identity.start_time) {
        return Ok(false);
    }
    let actual_executable = match fs::read_link(format!("/proc/{}/exe", identity.pid)) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(PlatformError::ProbeFailed(format!(
                "read process executable: {error}"
            )))
        }
    };
    if actual_executable != executable {
        return Ok(false);
    }
    Ok(read_cmdline(identity.pid)? == argv)
}

fn read_cmdline(pid: u32) -> Result<Vec<Vec<u8>>, PlatformError> {
    let bytes = match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(PlatformError::ProbeFailed(format!(
                "read process command line: {error}"
            )))
        }
    };
    decode_cmdline(&bytes)
}

fn decode_cmdline(bytes: &[u8]) -> Result<Vec<Vec<u8>>, PlatformError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.last() != Some(&0) {
        return Err(PlatformError::ProbeFailed(
            "process command line lacks NUL terminator".to_owned(),
        ));
    }
    Ok(bytes[..bytes.len() - 1]
        .split(|byte| *byte == 0)
        .map(ToOwned::to_owned)
        .collect())
}

const MAX_PROCESS_FD_ENTRIES: usize = 4096;
const MAX_PROCESS_FDINFO_SIZE: usize = 4096;
const TUN_DEVICE_PATH: &str = "/dev/net/tun";

pub(crate) fn mihomo_process_holds_tun(core: CoreIdentity) -> Result<bool, PlatformError> {
    if process_start_time(core.pid)? != Some(core.start_time) {
        return Ok(false);
    }
    let fd_directory = PathBuf::from(format!("/proc/{}/fd", core.pid));
    let fdinfo_directory = PathBuf::from(format!("/proc/{}/fdinfo", core.pid));
    let found = process_holds_named_tun(&fd_directory, &fdinfo_directory)?;
    Ok(found && process_start_time(core.pid)? == Some(core.start_time))
}

fn process_holds_named_tun(
    fd_directory: &Path,
    fdinfo_directory: &Path,
) -> Result<bool, PlatformError> {
    let entries = match fs::read_dir(fd_directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(PlatformError::ProbeFailed(format!(
                "scan Mihomo file descriptors: {error}"
            )))
        }
    };
    let expected = format!("iff:\t{MIHOMO_TUN_INTERFACE}");
    for (index, entry) in entries.enumerate() {
        if index >= MAX_PROCESS_FD_ENTRIES {
            return Err(PlatformError::ProbeFailed(
                "Mihomo file descriptor count exceeds its bound".to_owned(),
            ));
        }
        let entry = entry.map_err(|error| {
            PlatformError::ProbeFailed(format!("inspect Mihomo file descriptor: {error}"))
        })?;
        let target = match fs::read_link(entry.path()) {
            Ok(target) => target,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "inspect Mihomo file descriptor target: {error}"
                )))
            }
        };
        if target != Path::new(TUN_DEVICE_PATH) {
            continue;
        }
        let file = match File::open(fdinfo_directory.join(entry.file_name())) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "open Mihomo TUN fdinfo: {error}"
                )))
            }
        };
        let mut bytes = Vec::with_capacity(512);
        file.take((MAX_PROCESS_FDINFO_SIZE + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                PlatformError::ProbeFailed(format!("read Mihomo TUN fdinfo: {error}"))
            })?;
        if bytes.len() > MAX_PROCESS_FDINFO_SIZE {
            return Err(PlatformError::ProbeFailed(
                "Mihomo TUN fdinfo exceeds its bound".to_owned(),
            ));
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| PlatformError::ProbeFailed("Mihomo TUN fdinfo is not UTF-8".to_owned()))?;
        if text.lines().any(|line| line == expected) {
            return Ok(true);
        }
    }
    Ok(false)
}

const MAX_PROC_NET_TCP_SIZE: usize = 4 * 1024 * 1024;
const TCP_LISTEN_STATE: &str = "0A";

pub(crate) fn mihomo_process_owns_tcp_listener(
    core: CoreIdentity,
    address: SocketAddr,
) -> Result<bool, PlatformError> {
    if process_start_time(core.pid)? != Some(core.start_time) {
        return Ok(false);
    }
    let SocketAddr::V4(address) = address else {
        return Err(PlatformError::InvalidState(
            "Mihomo mixed listener address must be IPv4".to_owned(),
        ));
    };
    let listeners = tcp_listener_inodes(Path::new("/proc/net/tcp"), address)?;
    if listeners.len() != 1 {
        return Ok(false);
    }
    let sockets = process_socket_inodes(core.pid)?;
    Ok(process_start_time(core.pid)? == Some(core.start_time)
        && listeners.iter().all(|inode| sockets.contains(inode)))
}

fn tcp_listener_inodes(path: &Path, address: SocketAddrV4) -> Result<BTreeSet<u64>, PlatformError> {
    let file = File::open(path)
        .map_err(|error| PlatformError::ProbeFailed(format!("open TCP socket table: {error}")))?;
    let mut bytes = Vec::with_capacity(64 * 1024);
    file.take((MAX_PROC_NET_TCP_SIZE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| PlatformError::ProbeFailed(format!("read TCP socket table: {error}")))?;
    if bytes.len() > MAX_PROC_NET_TCP_SIZE {
        return Err(PlatformError::ProbeFailed(
            "TCP socket table exceeds its bound".to_owned(),
        ));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| PlatformError::ProbeFailed("TCP socket table is not UTF-8".to_owned()))?;
    let expected_address = u32::from_le_bytes(address.ip().octets());
    let mut inodes = BTreeSet::new();
    for line in text.lines().skip(1) {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() <= 9 || fields[3] != TCP_LISTEN_STATE {
            continue;
        }
        let Some((host, port)) = fields[1].split_once(':') else {
            continue;
        };
        let (Ok(host), Ok(port), Ok(inode)) = (
            u32::from_str_radix(host, 16),
            u16::from_str_radix(port, 16),
            fields[9].parse::<u64>(),
        ) else {
            continue;
        };
        if host == expected_address && port == address.port() {
            inodes.insert(inode);
        }
    }
    Ok(inodes)
}

fn process_socket_inodes(pid: u32) -> Result<BTreeSet<u64>, PlatformError> {
    let directory = PathBuf::from(format!("/proc/{pid}/fd"));
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => {
            return Err(PlatformError::ProbeFailed(format!(
                "scan Mihomo file descriptors: {error}"
            )))
        }
    };
    let mut inodes = BTreeSet::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_PROCESS_FD_ENTRIES {
            return Err(PlatformError::ProbeFailed(
                "Mihomo file descriptor count exceeds its bound".to_owned(),
            ));
        }
        let entry = entry.map_err(|error| {
            PlatformError::ProbeFailed(format!("inspect Mihomo file descriptor: {error}"))
        })?;
        let target = match fs::read_link(entry.path()) {
            Ok(target) => target,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "inspect Mihomo file descriptor target: {error}"
                )))
            }
        };
        let Some(target) = target.to_str() else {
            continue;
        };
        let Some(inode) = target
            .strip_prefix("socket:[")
            .and_then(|value| value.strip_suffix(']'))
            .and_then(|value| value.parse::<u64>().ok())
        else {
            continue;
        };
        inodes.insert(inode);
    }
    Ok(inodes)
}

pub(crate) fn count_executable_processes(executable: &Path) -> Result<usize, PlatformError> {
    let entries = fs::read_dir("/proc")
        .map_err(|error| PlatformError::ProbeFailed(format!("scan /proc: {error}")))?;
    let mut count = 0;
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
            .is_none()
        {
            continue;
        }
        match fs::read_link(entry.path().join("exe")) {
            Ok(actual) if actual == executable => count += 1,
            Ok(_) | Err(_) => {}
        }
    }
    Ok(count)
}

fn finish_mihomo_stop(tun_owned: bool) -> Result<(), PlatformError> {
    if mihomo_tun_ifindex()?.is_some() {
        return Err(PlatformError::Conflict(
            "hyz-mihomo remained or was replaced after exact core exit".to_owned(),
        ));
    }
    if tun_owned {
        super::storage::remove_file_durable(MIHOMO_TUN_IDENTITY)?;
    }
    super::storage::remove_file_durable(MIHOMO_PID_RECORD)?;
    remove_mihomo_runtime_credentials()
}

fn remove_mihomo_runtime_credentials() -> Result<(), PlatformError> {
    remove_mihomo_runtime_credentials_with(super::storage::remove_file_durable)
}

fn remove_mihomo_runtime_credentials_with(
    mut remove: impl FnMut(&str) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    let secret_result = remove(MIHOMO_CONTROLLER_SECRET);
    let runtime_result = remove(MIHOMO_RUNTIME_CONFIG);
    secret_result.and(runtime_result)
}

fn private_log(path: &str) -> Result<File, PlatformError> {
    let mut options = OpenOptions::new();
    options.write(true).truncate(true).mode(0o600);
    match fs::symlink_metadata(path) {
        Ok(_) => {
            super::storage::require_private_root_file(path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            options.create_new(true);
        }
        Err(error) => {
            return Err(PlatformError::Io(format!(
                "inspect private log {path}: {error}"
            )))
        }
    }
    options
        .open(path)
        .map_err(|error| PlatformError::Io(format!("open private log {path}: {error}")))
}

pub(crate) fn append_fail_open_retry_log(message: &str) -> Result<(), PlatformError> {
    const MAX_LOG_SIZE: u64 = 64 * 1024;

    super::storage::ensure_private_dir(MIHOMO_STATE_DIR)?;
    let mut options = OpenOptions::new();
    options.write(true).append(true).mode(0o600);
    match fs::symlink_metadata(MIHOMO_WATCHER_LOG) {
        Ok(_) => super::storage::require_private_root_file(MIHOMO_WATCHER_LOG)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            options.create_new(true);
        }
        Err(error) => {
            return Err(PlatformError::Io(format!(
                "inspect private watcher log: {error}"
            )))
        }
    }
    let mut log = options
        .open(MIHOMO_WATCHER_LOG)
        .map_err(|error| PlatformError::Io(format!("open private watcher log: {error}")))?;
    let clean = message
        .chars()
        .take(4096)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    if log
        .metadata()
        .map_err(|error| PlatformError::Io(format!("inspect watcher log size: {error}")))?
        .len()
        .saturating_add(clean.len() as u64 + 1)
        > MAX_LOG_SIZE
    {
        log.set_len(0)
            .map_err(|error| PlatformError::Io(format!("truncate watcher log: {error}")))?;
    }
    writeln!(log, "{clean}")
        .map_err(|error| PlatformError::Io(format!("append watcher log: {error}")))
}

pub(crate) fn wait_for_identity_match(
    attempts: usize,
    poll: Duration,
    mut identity_matches: impl FnMut() -> Result<bool, PlatformError>,
) -> Result<bool, PlatformError> {
    let mut last_error = None;
    for attempt in 0..attempts {
        match identity_matches() {
            Ok(true) => return Ok(true),
            Ok(false) => last_error = None,
            Err(error) => last_error = Some(error),
        }
        if attempt + 1 < attempts && !poll.is_zero() {
            thread::sleep(poll);
        }
    }
    match last_error {
        Some(error) => Err(error),
        None => Ok(false),
    }
}

pub(crate) fn wait_for_identity_disappearance(
    attempts: usize,
    poll: Duration,
    mut identity_matches: impl FnMut() -> Result<bool, PlatformError>,
) -> Result<bool, PlatformError> {
    for attempt in 0..attempts {
        if !identity_matches()? {
            return Ok(true);
        }
        if attempt + 1 < attempts && !poll.is_zero() {
            thread::sleep(poll);
        }
    }
    Ok(false)
}

fn wait_command(
    child: &mut std::process::Child,
    timeout: Duration,
    label: &str,
) -> Result<(), PlatformError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| PlatformError::Io(format!("wait for {label}: {error}")))?
        {
            return if status.success() {
                Ok(())
            } else {
                Err(PlatformError::CommandFailed(format!(
                    "{label} failed; inspect {MIHOMO_CHECK_LOG}"
                )))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PlatformError::CommandFailed(format!("{label} timed out")));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn encode_argv(argv: &[Vec<u8>]) -> String {
    argv.iter()
        .map(|arg| {
            let mut encoded = String::with_capacity(arg.len() * 2);
            for byte in arg {
                use std::fmt::Write as _;
                let _ = write!(encoded, "{byte:02x}");
            }
            encoded
        })
        .collect::<Vec<_>>()
        .join(":")
}

fn decode_argv(encoded: &str) -> Result<Vec<Vec<u8>>, PlatformError> {
    encoded
        .split(':')
        .map(|arg| {
            if arg.is_empty() || arg.len() % 2 != 0 {
                return Err(PlatformError::InvalidState(
                    "identity argv encoding is invalid".to_owned(),
                ));
            }
            (0..arg.len())
                .step_by(2)
                .map(|index| {
                    u8::from_str_radix(&arg[index..index + 2], 16).map_err(|_| {
                        PlatformError::InvalidState("identity argv hex is invalid".to_owned())
                    })
                })
                .collect()
        })
        .collect()
}

fn parse_u32(value: Option<&str>, label: &str) -> Result<u32, PlatformError> {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| PlatformError::InvalidState(format!("invalid {label} record")))
}

fn parse_u64(value: Option<&str>, label: &str) -> Result<u64, PlatformError> {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| PlatformError::InvalidState(format!("invalid {label} record")))
}

fn parse_sha256(value: Option<&str>) -> Result<String, PlatformError> {
    let value = value.ok_or_else(|| {
        PlatformError::InvalidState("runtime config SHA-256 record is absent".to_owned())
    })?;
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PlatformError::InvalidState(
            "runtime config SHA-256 record is invalid".to_owned(),
        ));
    }
    Ok(value.to_ascii_lowercase())
}

pub(crate) fn reap_in_background(mut child: std::process::Child, label: &'static str) {
    std::thread::Builder::new()
        .name(format!("hyz-reap-{label}"))
        .spawn(move || {
            let _ = child.wait();
        })
        .expect("daemon must be able to retain a child reaper");
}

pub(crate) fn process_start_time(pid: u32) -> Result<Option<u64>, PlatformError> {
    Ok(process_stat_identity(pid)?.map(|identity| identity.0))
}

pub(crate) fn process_stat_identity(pid: u32) -> Result<Option<(u64, u8)>, PlatformError> {
    let stat = match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(PlatformError::ProbeFailed(format!(
                "read process stat: {error}"
            )))
        }
    };
    let end = stat.rfind(')').ok_or_else(|| {
        PlatformError::ProbeFailed("malformed process stat without comm terminator".to_owned())
    })?;
    let mut fields = stat[end + 1..].split_whitespace();
    let state = fields
        .next()
        .and_then(|value| value.as_bytes().first().copied())
        .ok_or_else(|| PlatformError::ProbeFailed("malformed process state".to_owned()))?;
    let start_time = fields
        .nth(18)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| PlatformError::ProbeFailed("malformed process start time".to_owned()))?;
    Ok(Some((start_time, state)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn bounded_command_stream_accepts_the_limit_and_rejects_one_extra_byte() {
        assert_eq!(
            read_bounded_command_stream(std::io::Cursor::new(vec![b'x'; 64]), 64).unwrap(),
            vec![b'x'; 64]
        );
        assert!(read_bounded_command_stream(std::io::Cursor::new(vec![b'x'; 65]), 64).is_err());
    }

    #[test]
    fn tun_probe_ignores_large_fdinfo_for_unrelated_descriptors() {
        use std::os::unix::fs::symlink;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "hyz-router-tun-probe-{}-{unique}",
            std::process::id()
        ));
        let fd = root.join("fd");
        let fdinfo = root.join("fdinfo");
        fs::create_dir_all(&fd).unwrap();
        fs::create_dir_all(&fdinfo).unwrap();
        symlink("/dev/null", fd.join("7")).unwrap();
        fs::write(fdinfo.join("7"), vec![b'x'; MAX_PROCESS_FDINFO_SIZE + 1]).unwrap();
        symlink(TUN_DEVICE_PATH, fd.join("8")).unwrap();
        fs::write(
            fdinfo.join("8"),
            format!("pos:\t0\niff:\t{MIHOMO_TUN_INTERFACE}\n"),
        )
        .unwrap();

        assert!(process_holds_named_tun(&fd, &fdinfo).unwrap());

        fs::write(fdinfo.join("8"), "pos:\t0\niff:\tforeign-tun\n").unwrap();
        assert!(!process_holds_named_tun(&fd, &fdinfo).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tcp_listener_probe_matches_only_the_exact_single_loopback_listener() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "hyz-router-tcp-listener-{}-{unique}",
            std::process::id()
        ));
        fs::write(
            &path,
            "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1ED2 00000000:0000 0A 00000000:00000000 00:00000000 00000000 0 0 4242\n   1: 0100007F:1ED2 00000000:0000 01 00000000:00000000 00:00000000 00000000 0 0 9999\n   2: 00000000:1ED2 00000000:0000 0A 00000000:00000000 00:00000000 00000000 0 0 8888\n",
        )
        .unwrap();
        let address = "127.0.0.1:7890".parse::<SocketAddr>().unwrap();
        let SocketAddr::V4(address) = address else {
            unreachable!();
        };

        assert_eq!(
            tcp_listener_inodes(&path, address).unwrap(),
            BTreeSet::from([4242])
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn empty_zombie_cmdline_is_a_non_match_not_a_probe_error() {
        assert_eq!(decode_cmdline(&[]).unwrap(), Vec::<Vec<u8>>::new());
        assert!(decode_cmdline(b"/usr/bin/mihomo").is_err());
        assert_eq!(
            decode_cmdline(b"/usr/bin/mihomo\0-d\0/data\0").unwrap(),
            [
                b"/usr/bin/mihomo".to_vec(),
                b"-d".to_vec(),
                b"/data".to_vec()
            ]
        );
    }

    #[test]
    fn normal_stop_removes_secret_and_runtime_config() {
        let mut removed = Vec::new();
        remove_mihomo_runtime_credentials_with(|path| {
            removed.push(path.to_owned());
            Ok(())
        })
        .unwrap();

        assert_eq!(removed, [MIHOMO_CONTROLLER_SECRET, MIHOMO_RUNTIME_CONFIG]);
    }

    #[test]
    fn credential_cleanup_attempts_runtime_config_after_secret_error() {
        let mut removed = Vec::new();
        let result = remove_mihomo_runtime_credentials_with(|path| {
            removed.push(path.to_owned());
            if path == MIHOMO_CONTROLLER_SECRET {
                Err(PlatformError::Io("secret cleanup failed".to_owned()))
            } else {
                Ok(())
            }
        });

        assert!(result.is_err());
        assert_eq!(removed, [MIHOMO_CONTROLLER_SECRET, MIHOMO_RUNTIME_CONFIG]);
    }

    #[test]
    fn identity_match_retries_transient_probe_failure_and_incomplete_identity() {
        let mut probes = VecDeque::from([
            Err(PlatformError::ProbeFailed(
                "process command line lacks NUL terminator".to_owned(),
            )),
            Ok(false),
            Ok(true),
        ]);
        assert_eq!(
            wait_for_identity_match(3, Duration::ZERO, || probes.pop_front().unwrap()),
            Ok(true)
        );
    }

    #[test]
    fn identity_match_returns_final_probe_failure_after_bound() {
        assert!(matches!(
            wait_for_identity_match(2, Duration::ZERO, || Err(PlatformError::ProbeFailed(
                "unreadable identity".to_owned()
            ))),
            Err(PlatformError::ProbeFailed(_))
        ));
    }

    #[test]
    fn identity_record_is_removable_only_after_exact_identity_disappears() {
        let mut matches = VecDeque::from([true, true, false]);
        assert_eq!(
            wait_for_identity_disappearance(3, Duration::ZERO, || {
                Ok(matches.pop_front().unwrap())
            }),
            Ok(true)
        );
    }

    #[test]
    fn identity_wait_reports_still_live_after_bound() {
        let mut probes = 0;
        assert_eq!(
            wait_for_identity_disappearance(3, Duration::ZERO, || {
                probes += 1;
                Ok(true)
            }),
            Ok(false)
        );
        assert_eq!(probes, 3);
    }

    #[test]
    fn identity_wait_propagates_probe_failure_conservatively() {
        assert!(matches!(
            wait_for_identity_disappearance(1, Duration::ZERO, || Err(PlatformError::ProbeFailed(
                "unreadable identity".to_owned()
            ))),
            Err(PlatformError::ProbeFailed(_))
        ));
    }
}
