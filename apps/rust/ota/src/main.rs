use sha2::{Digest, Sha256};
use std::env;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const DEFAULT_DOWNLOAD: &str = "/userdata/upgrade.fw";
const UPDATE_ENGINE: &str = "/usr/bin/updateEngine";
const REBOOT: &str = "/sbin/reboot";
const MISC_PARTITION: &str = "/dev/block/by-name/misc";
const BCB_OFFSET: u64 = 16 * 1024;
const BCB_SIZE: usize = 1088;
const COMMAND_SIZE: usize = 32;
const STATUS_SIZE: usize = 32;
const RECOVERY_SIZE: usize = 768;
const NEEDUPDATE_OFFSET: usize = COMMAND_SIZE + STATUS_SIZE + RECOVERY_SIZE;
// This is the mask Rockchip updateEngine writes after it has removed the
// recovery bit from the default non-A/B update mask.
const RECOVERY_UPDATE_MASK: u32 = 0x003b_0000;

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hyz-ota: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    match args.as_slice() {
        [command, file, expected] if command == "verify" => {
            verify_firmware(Path::new(file), expected)?;
            println!("verified {file}");
        }
        [command, url, expected] if command == "download" => {
            let destination = Path::new(DEFAULT_DOWNLOAD);
            download(url, destination)?;
            verify_firmware(destination, expected)?;
            println!("downloaded and verified {}", destination.display());
        }
        [command, url, expected, destination] if command == "download" => {
            let destination = Path::new(destination);
            download(url, destination)?;
            verify_firmware(destination, expected)?;
            println!("downloaded and verified {}", destination.display());
        }
        [command, file, expected] if command == "install" => {
            install(Path::new(file), expected, false, false)?;
        }
        [command, file, expected, flag] if command == "install" && flag == "--reboot" => {
            install(Path::new(file), expected, true, false)?;
        }
        [command, file, expected] if command == "install-recovery" => {
            install(Path::new(file), expected, false, true)?;
        }
        [command, file, expected, flag] if command == "install-recovery" && flag == "--reboot" => {
            install(Path::new(file), expected, true, true)?;
        }
        [command, url, expected] if command == "apply" => {
            apply(url, expected, false)?;
        }
        [command, url, expected, flag] if command == "apply" && flag == "--reboot" => {
            apply(url, expected, true)?;
        }
        _ => usage(),
    }
    Ok(())
}

fn usage() {
    eprintln!(
        "Usage:\n  hyz-ota verify <upgrade.fw> <sha256>\n  hyz-ota download <http(s)://url> <sha256> [destination]\n  hyz-ota install <recovery-free.fw> <sha256> [--reboot]\n  hyz-ota install-recovery <recovery.fw> <sha256> [--reboot]\n  hyz-ota apply <http(s)://url> <sha256> [--reboot]"
    );
}

fn apply(url: &str, expected: &str, reboot: bool) -> Result<(), Box<dyn Error>> {
    let destination = Path::new(DEFAULT_DOWNLOAD);
    download(url, destination)?;
    install(destination, expected, reboot, false)
}

fn download(url: &str, destination: &Path) -> Result<(), Box<dyn Error>> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("only HTTP and HTTPS URLs are accepted".into());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    let temporary = temporary_path(destination);
    let mut response = ureq::get(url).call()?;
    let mut source = response.body_mut().as_reader();
    let mut target = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
    io::copy(&mut source, &mut target)?;
    target.flush()?;
    target.sync_all()?;
    fs::rename(&temporary, destination)?;
    Ok(())
}

fn temporary_path(destination: &Path) -> PathBuf {
    let mut value = destination.as_os_str().to_owned();
    value.push(".part");
    PathBuf::from(value)
}

fn install(
    file: &Path,
    expected: &str,
    reboot: bool,
    update_recovery: bool,
) -> Result<(), Box<dyn Error>> {
    verify_firmware(file, expected)?;
    let canonical = fs::canonicalize(file)?;
    File::open(&canonical)?.sync_all()?;

    if update_recovery {
        stage_with_update_engine(&canonical)?;
    } else {
        stage_recovery_free(&canonical, Path::new(MISC_PARTITION))?;
    }

    println!("staged verified firmware {}", canonical.display());
    if reboot {
        request_reboot()?;
    }
    Ok(())
}

fn stage_with_update_engine(file: &Path) -> Result<(), Box<dyn Error>> {
    if !Path::new(UPDATE_ENGINE).is_file() {
        return Err(format!("Rockchip update engine is missing at {UPDATE_ENGINE}").into());
    }
    let status = Command::new(UPDATE_ENGINE)
        .arg(format!("--image_url={}", file.display()))
        .arg("--update")
        .status()?;
    if !status.success() {
        return Err(format!("updateEngine exited with {status}").into());
    }
    verify_pending_bcb(file, Path::new(MISC_PARTITION))
}

fn stage_recovery_free(file: &Path, misc_path: &Path) -> Result<(), Box<dyn Error>> {
    if !Path::new("/dev/block/by-name/recovery").exists() && misc_path == Path::new(MISC_PARTITION)
    {
        return Err("recovery partition is missing".into());
    }
    let message = build_bootloader_message(file)?;
    let mut misc = OpenOptions::new().read(true).write(true).open(misc_path)?;
    let mut current = [0_u8; BCB_SIZE];
    misc.seek(SeekFrom::Start(BCB_OFFSET))?;
    misc.read_exact(&mut current)?;
    if current[..COMMAND_SIZE].iter().any(|byte| *byte != 0) {
        return Err("misc already contains a pending bootloader command".into());
    }

    misc.seek(SeekFrom::Start(BCB_OFFSET))?;
    misc.write_all(&message)?;
    misc.flush()?;
    misc.sync_all()?;
    verify_pending_bcb_with_handle(file, &mut misc)
}

fn verify_pending_bcb(file: &Path, misc_path: &Path) -> Result<(), Box<dyn Error>> {
    let mut misc = OpenOptions::new().read(true).open(misc_path)?;
    verify_pending_bcb_with_handle(file, &mut misc)
}

fn verify_pending_bcb_with_handle(file: &Path, misc: &mut File) -> Result<(), Box<dyn Error>> {
    let expected = build_bootloader_message(file)?;
    let mut actual = [0_u8; BCB_SIZE];
    misc.seek(SeekFrom::Start(BCB_OFFSET))?;
    misc.read_exact(&mut actual)?;
    if actual != expected {
        return Err("misc BCB verification failed after staging".into());
    }
    Ok(())
}

fn build_bootloader_message(file: &Path) -> Result<[u8; BCB_SIZE], Box<dyn Error>> {
    let path = file.to_str().ok_or("firmware path must be valid UTF-8")?;
    if path.contains(['\0', '\n', '\r']) {
        return Err("firmware path contains an unsupported control character".into());
    }

    let recovery = format!("recovery\n--update_package={path}\n");
    if recovery.len() >= RECOVERY_SIZE {
        return Err("firmware path is too long for the recovery BCB".into());
    }

    let mut message = [0_u8; BCB_SIZE];
    message[.."boot-recovery".len()].copy_from_slice(b"boot-recovery");
    let recovery_offset = COMMAND_SIZE + STATUS_SIZE;
    message[recovery_offset..recovery_offset + recovery.len()].copy_from_slice(recovery.as_bytes());
    message[NEEDUPDATE_OFFSET..NEEDUPDATE_OFFSET + 4]
        .copy_from_slice(&RECOVERY_UPDATE_MASK.to_le_bytes());
    Ok(message)
}

fn request_reboot() -> Result<(), Box<dyn Error>> {
    let status = Command::new(REBOOT).status()?;
    if !status.success() {
        return Err(format!("reboot command exited with {status}").into());
    }
    Ok(())
}

fn verify_firmware(file: &Path, expected: &str) -> Result<(), Box<dyn Error>> {
    let expected = normalize_digest(expected)?;
    let mut input = File::open(file)?;
    let mut magic = [0_u8; 4];
    input.read_exact(&mut magic)?;
    if &magic != b"RKFW" {
        return Err(format!("{} is not a Rockchip RKFW image", file.display()).into());
    }

    let actual = sha256(file)?;
    if actual != expected {
        return Err(format!("SHA-256 mismatch: expected {expected}, got {actual}").into());
    }
    Ok(())
}

fn normalize_digest(value: &str) -> Result<String, Box<dyn Error>> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("SHA-256 must be exactly 64 hexadecimal characters".into());
    }
    Ok(value)
}

fn sha256(path: &Path) -> Result<String, io::Error> {
    let mut input = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let bytes = digest.finalize();
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::{
        build_bootloader_message, normalize_digest, stage_recovery_free, BCB_OFFSET, BCB_SIZE,
        NEEDUPDATE_OFFSET, RECOVERY_UPDATE_MASK,
    };
    use std::fs::{self, File};
    use std::io::{Read, Seek, SeekFrom};
    use std::path::Path;

    #[test]
    fn accepts_uppercase_sha256() {
        let value = "A".repeat(64);
        assert_eq!(normalize_digest(&value).unwrap(), "a".repeat(64));
    }

    #[test]
    fn rejects_invalid_sha256() {
        assert!(normalize_digest("not-a-digest").is_err());
    }

    #[test]
    fn builds_rockchip_recovery_free_bcb() {
        let message = build_bootloader_message(Path::new("/userdata/upgrade.fw")).unwrap();
        assert_eq!(&message[..13], b"boot-recovery");
        let recovery = b"recovery\n--update_package=/userdata/upgrade.fw\n";
        assert_eq!(&message[64..64 + recovery.len()], recovery);
        assert_eq!(
            &message[NEEDUPDATE_OFFSET..NEEDUPDATE_OFFSET + 4],
            &RECOVERY_UPDATE_MASK.to_le_bytes()
        );
        assert!(message[64 + recovery.len()..NEEDUPDATE_OFFSET]
            .iter()
            .all(|byte| *byte == 0));
    }

    #[test]
    fn stages_and_verifies_bcb_at_rockchip_misc_offset() {
        let path = std::env::temp_dir().join(format!("hyz-ota-misc-{}", std::process::id()));
        let misc = File::create(&path).unwrap();
        misc.set_len(BCB_OFFSET + BCB_SIZE as u64).unwrap();
        drop(misc);

        let firmware = Path::new("/userdata/upgrade.fw");
        stage_recovery_free(firmware, &path).unwrap();

        let mut misc = File::open(&path).unwrap();
        misc.seek(SeekFrom::Start(BCB_OFFSET)).unwrap();
        let mut actual = [0_u8; BCB_SIZE];
        misc.read_exact(&mut actual).unwrap();
        assert_eq!(actual, build_bootloader_message(firmware).unwrap());
        assert!(stage_recovery_free(firmware, &path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_control_characters_in_bcb_path() {
        assert!(build_bootloader_message(Path::new("/userdata/bad\npath.fw")).is_err());
    }

    #[test]
    fn rejects_bcb_path_that_is_too_long() {
        let path = format!("/userdata/{}.fw", "a".repeat(760));
        assert!(build_bootloader_message(Path::new(&path)).is_err());
    }
}
