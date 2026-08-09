use super::paths::{
    MIHOMO_CHECK_LOG, MIHOMO_CONTROLLER_SECRET, MIHOMO_DATA_DIR, MIHOMO_EXECUTABLE, MIHOMO_LOG,
    MIHOMO_RUNTIME_CONFIG, MIHOMO_STATE_DIR, MIHOMO_WATCHER_LOG,
};
use crate::application::{
    fail_open::WatcherInvocation,
    ports::{CoreIdentity, CoreRecordState, PlatformError},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const NETWORK_LOCK: &str = "/run/hyz-network.lock";
pub const MIHOMO_PID_RECORD: &str = "/run/hyz-mihomo/core.pid";
pub const MIHOMO_WATCHER_RECORD: &str = "/run/hyz-mihomo/watch.pid";
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

    pub(crate) fn run(&self, tool: Tool, args: &[String]) -> Result<FixedOutput, PlatformError> {
        let output = self.run_probe(tool, args)?;
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
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        loop {
            if child
                .try_wait()
                .map_err(|error| PlatformError::Io(format!("could not wait for command: {error}")))?
                .is_some()
            {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PlatformError::CommandFailed(
                    "fixed command exceeded three-second deadline".to_owned(),
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
        let output = child
            .wait_with_output()
            .map_err(|error| PlatformError::Io(format!("could not collect command: {error}")))?;
        if output.stdout.len() + output.stderr.len() > MAX_COMMAND_OUTPUT {
            return Err(PlatformError::CommandFailed(
                "fixed command output exceeded limit".to_owned(),
            ));
        }
        let success = output.status.success();
        let stdout = String::from_utf8(output.stdout).map_err(|_| {
            PlatformError::CommandFailed("fixed command emitted non-UTF-8 stdout".to_owned())
        })?;
        let stderr = String::from_utf8(output.stderr).map_err(|_| {
            PlatformError::CommandFailed("fixed command emitted non-UTF-8 stderr".to_owned())
        })?;
        Ok(FixedOutput {
            success,
            stdout,
            stderr,
        })
    }

    pub(crate) fn validate_mihomo_config(&self) -> Result<(), PlatformError> {
        super::storage::ensure_private_dir(MIHOMO_STATE_DIR)?;
        let log = private_log(MIHOMO_CHECK_LOG)?;
        let stderr = log
            .try_clone()
            .map_err(|error| PlatformError::Io(format!("clone config-check log: {error}")))?;
        let mut child = Command::new(Tool::Mihomo.path())
            .args(["-t", "-d", MIHOMO_DATA_DIR, "-f", MIHOMO_RUNTIME_CONFIG])
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
            return remove_mihomo_runtime_credentials();
        };
        self.run(
            Tool::Kill,
            &["-TERM".to_owned(), identity.core.pid.to_string()],
        )?;
        if wait_for_identity_disappearance(IDENTITY_EXIT_ATTEMPTS, IDENTITY_EXIT_POLL, || {
            identity.matches_process_only()
        })? {
            super::storage::remove_file_durable(MIHOMO_PID_RECORD)?;
            remove_mihomo_runtime_credentials()?;
            return Ok(());
        }
        self.run(
            Tool::Kill,
            &["-KILL".to_owned(), identity.core.pid.to_string()],
        )?;
        if wait_for_identity_disappearance(IDENTITY_EXIT_ATTEMPTS, IDENTITY_EXIT_POLL, || {
            identity.matches_process_only()
        })? {
            super::storage::remove_file_durable(MIHOMO_PID_RECORD)?;
            remove_mihomo_runtime_credentials()
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

fn process_identity_matches(
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

fn count_executable_processes(executable: &Path) -> Result<usize, PlatformError> {
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

fn wait_for_identity_disappearance(
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
    let start_time = stat[end + 1..]
        .split_whitespace()
        .nth(19)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| PlatformError::ProbeFailed("malformed process start time".to_owned()))?;
    Ok(Some(start_time))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

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
