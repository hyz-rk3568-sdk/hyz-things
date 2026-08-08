use sha2::{Digest, Sha256};
use std::env;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const DEFAULT_DOWNLOAD: &str = "/userdata/upgrade.fw";
const UPDATE_ENGINE: &str = "/usr/bin/updateEngine";

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
            println!("verified {}", file);
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
            install(Path::new(file), expected, false)?;
        }
        [command, file, expected, flag] if command == "install" && flag == "--reboot" => {
            install(Path::new(file), expected, true)?;
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
        "Usage:\n  hyz-ota verify <upgrade.fw> <sha256>\n  hyz-ota download <http(s)://url> <sha256> [destination]\n  hyz-ota install <upgrade.fw> <sha256> [--reboot]\n  hyz-ota apply <http(s)://url> <sha256> [--reboot]"
    );
}

fn apply(url: &str, expected: &str, reboot: bool) -> Result<(), Box<dyn Error>> {
    let destination = Path::new(DEFAULT_DOWNLOAD);
    download(url, destination)?;
    install(destination, expected, reboot)
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

fn install(file: &Path, expected: &str, reboot: bool) -> Result<(), Box<dyn Error>> {
    verify_firmware(file, expected)?;
    let canonical = fs::canonicalize(file)?;
    if !Path::new(UPDATE_ENGINE).is_file() {
        return Err(format!("Rockchip update engine is missing at {UPDATE_ENGINE}").into());
    }

    let mut command = Command::new(UPDATE_ENGINE);
    command.arg(format!("--image_url={}", canonical.display()));
    command.arg("--update");
    if reboot {
        command.arg("--reboot");
    }

    println!("installing verified firmware {}", canonical.display());
    let status = command.status()?;
    if !status.success() {
        return Err(format!("updateEngine exited with {status}").into());
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
    use super::normalize_digest;

    #[test]
    fn accepts_uppercase_sha256() {
        let value = "A".repeat(64);
        assert_eq!(normalize_digest(&value).unwrap(), "a".repeat(64));
    }

    #[test]
    fn rejects_invalid_sha256() {
        assert!(normalize_digest("not-a-digest").is_err());
    }
}
