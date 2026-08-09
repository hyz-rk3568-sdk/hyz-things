#![cfg(feature = "native")]

use hyz_router::adapters::inbound::ota_cli::{parse_ota_cli, OtaCommand};
use hyz_router::adapters::outbound::firmware::{FirmwareAdapter, FirmwareAdapterConfig};
use hyz_router::application::ota::{
    FirmwareIdentity, FirmwareMutationLock, FirmwarePlatformPort, InstallMode, OtaService,
    PlatformError, TrustedFirmware,
};
use hyz_router::domain::ota::{
    ensure_no_pending_command, verify_bcb_exact, BootloaderMessage, OtaDomainError, Sha256Digest,
    BCB_NEED_UPDATE_OFFSET, BCB_OFFSET, BCB_SIZE, RECOVERY_UPDATE_MASK,
};
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

const EXPECTED_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TEMPORARY: &str = "/staging/.upgrade.part";
const COMMITTED: &str = "/staging/upgrade.fw";

#[derive(Clone)]
struct FakeFirmwarePlatform {
    calls: Rc<RefCell<Vec<String>>>,
    bcb: Rc<RefCell<Vec<u8>>>,
    prefix: Vec<u8>,
    digest: [u8; 32],
    recovery_exists: bool,
}

impl Default for FakeFirmwarePlatform {
    fn default() -> Self {
        Self {
            calls: Rc::new(RefCell::new(Vec::new())),
            bcb: Rc::new(RefCell::new(vec![0_u8; BCB_SIZE])),
            prefix: b"RKFW".to_vec(),
            digest: [0xaa; 32],
            recovery_exists: true,
        }
    }
}

impl FakeFirmwarePlatform {
    fn call(&self, value: impl Into<String>) {
        self.calls.borrow_mut().push(value.into());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

struct FakeFirmware {
    path: PathBuf,
    identity: FirmwareIdentity,
}

impl FakeFirmware {
    fn new(path: impl Into<PathBuf>, inode: u64) -> Self {
        Self {
            path: path.into(),
            identity: FirmwareIdentity {
                device: 1,
                inode,
                size: 4096,
            },
        }
    }
}

impl TrustedFirmware for FakeFirmware {
    fn path(&self) -> &Path {
        &self.path
    }

    fn identity(&self) -> FirmwareIdentity {
        self.identity
    }
}

struct FakeMutationLock {
    calls: Rc<RefCell<Vec<String>>>,
}

impl FirmwareMutationLock for FakeMutationLock {}

impl Drop for FakeMutationLock {
    fn drop(&mut self) {
        self.calls.borrow_mut().push("unlock".to_owned());
    }
}

impl FirmwarePlatformPort for FakeFirmwarePlatform {
    type Firmware = FakeFirmware;

    fn acquire_mutation_lock(&self) -> Result<Box<dyn FirmwareMutationLock + '_>, PlatformError> {
        self.call("lock");
        Ok(Box::new(FakeMutationLock {
            calls: Rc::clone(&self.calls),
        }))
    }

    fn open_firmware(&self, path: &Path) -> Result<Self::Firmware, PlatformError> {
        self.call(format!("open:{}", path.to_string_lossy()));
        Ok(FakeFirmware::new(path, 10))
    }

    fn copy_to_staging_temporary(
        &self,
        source: &mut Self::Firmware,
    ) -> Result<Self::Firmware, PlatformError> {
        self.call(format!("copy:{}", source.path.to_string_lossy()));
        Ok(FakeFirmware::new(TEMPORARY, 20))
    }

    fn download_to_staging_temporary(
        &self,
        _source: &str,
    ) -> Result<Self::Firmware, PlatformError> {
        self.call("download");
        Ok(FakeFirmware::new(TEMPORARY, 20))
    }

    fn commit_staged(&self, firmware: &mut Self::Firmware) -> Result<(), PlatformError> {
        self.call(format!("commit:{}", firmware.path.to_string_lossy()));
        firmware.path = PathBuf::from(COMMITTED);
        Ok(())
    }

    fn discard_staged(&self, firmware: &Self::Firmware) -> Result<(), PlatformError> {
        self.call(format!("discard:{}", firmware.path.to_string_lossy()));
        Ok(())
    }

    fn read_firmware_prefix(
        &self,
        firmware: &mut Self::Firmware,
    ) -> Result<Vec<u8>, PlatformError> {
        self.call(format!("prefix:{}", firmware.path.to_string_lossy()));
        Ok(self.prefix.clone())
    }

    fn sha256(&self, firmware: &mut Self::Firmware) -> Result<[u8; 32], PlatformError> {
        self.call(format!("sha256:{}", firmware.path.to_string_lossy()));
        Ok(self.digest)
    }

    fn sync_firmware(&self, firmware: &Self::Firmware) -> Result<(), PlatformError> {
        self.call(format!("sync:{}", firmware.path.to_string_lossy()));
        Ok(())
    }

    fn revalidate_firmware(&self, firmware: &Self::Firmware) -> Result<(), PlatformError> {
        self.call(format!(
            "revalidate:{}:{}:{}:{}",
            firmware.path.to_string_lossy(),
            firmware.identity.device,
            firmware.identity.inode,
            firmware.identity.size
        ));
        Ok(())
    }

    fn recovery_partition_exists(&self) -> Result<bool, PlatformError> {
        self.call("recovery-exists");
        Ok(self.recovery_exists)
    }

    fn read_bcb(&self) -> Result<Vec<u8>, PlatformError> {
        self.call("read-bcb");
        Ok(self.bcb.borrow().clone())
    }

    fn commit_recovery_update(
        &self,
        firmware: &Self::Firmware,
        message: &BootloaderMessage,
    ) -> Result<(), PlatformError> {
        self.call(format!(
            "commit-bcb:{}:{}",
            firmware.path.to_string_lossy(),
            firmware.identity.inode
        ));
        *self.bcb.borrow_mut() = message.as_bytes().to_vec();
        Ok(())
    }

    fn stage_with_update_engine(&self, firmware: &Self::Firmware) -> Result<(), PlatformError> {
        self.call(format!(
            "update-engine:{}:{}",
            firmware.path.to_string_lossy(),
            firmware.identity.inode
        ));
        let path = firmware
            .path
            .to_str()
            .ok_or_else(|| PlatformError::new("test path was not UTF-8"))?;
        *self.bcb.borrow_mut() = BootloaderMessage::for_update_package(path)
            .map_err(|error| PlatformError::new(error.to_string()))?
            .as_bytes()
            .to_vec();
        Ok(())
    }

    fn reboot(&self) -> Result<(), PlatformError> {
        self.call("reboot");
        Ok(())
    }
}

fn validation(path: &str, inode: u64) -> String {
    format!("revalidate:{path}:1:{inode}:4096")
}

#[test]
fn download_verifies_open_descriptor_before_atomic_fixed_staging_commit() {
    let platform = FakeFirmwarePlatform::default();
    let inspect = platform.clone();
    let service = OtaService::new(platform);

    assert_eq!(
        service
            .download("https://example.invalid/upgrade.fw", EXPECTED_DIGEST)
            .unwrap(),
        PathBuf::from(COMMITTED)
    );

    assert_eq!(
        inspect.calls(),
        [
            "lock".to_owned(),
            "download".to_owned(),
            validation(TEMPORARY, 20),
            format!("prefix:{TEMPORARY}"),
            format!("sha256:{TEMPORARY}"),
            validation(TEMPORARY, 20),
            format!("commit:{TEMPORARY}"),
            validation(COMMITTED, 20),
            "unlock".to_owned(),
        ]
    );
}

#[test]
fn failed_download_verification_discards_temporary_before_unlocking() {
    let platform = FakeFirmwarePlatform {
        digest: [0xbb; 32],
        ..FakeFirmwarePlatform::default()
    };
    let inspect = platform.clone();
    let service = OtaService::new(platform);

    let error = service
        .download("https://example.invalid/upgrade.fw", EXPECTED_DIGEST)
        .unwrap_err();

    assert!(error.to_string().contains("SHA-256 mismatch"));
    assert_eq!(
        inspect.calls(),
        [
            "lock".to_owned(),
            "download".to_owned(),
            validation(TEMPORARY, 20),
            format!("prefix:{TEMPORARY}"),
            format!("sha256:{TEMPORARY}"),
            format!("discard:{TEMPORARY}"),
            "unlock".to_owned(),
        ]
    );
}

#[test]
fn install_copies_into_fixed_staging_before_verify_and_revalidates_before_bcb() {
    let platform = FakeFirmwarePlatform::default();
    let inspect = platform.clone();
    let service = OtaService::new(platform);

    service
        .install(
            Path::new("/incoming/upgrade.fw"),
            EXPECTED_DIGEST,
            InstallMode::RecoveryFree,
            true,
        )
        .unwrap();

    let calls = inspect.calls();
    assert_eq!(
        calls[0..3],
        [
            "lock",
            "open:/incoming/upgrade.fw",
            "copy:/incoming/upgrade.fw"
        ]
    );
    assert_eq!(
        calls,
        [
            "lock".to_owned(),
            "open:/incoming/upgrade.fw".to_owned(),
            "copy:/incoming/upgrade.fw".to_owned(),
            validation(TEMPORARY, 20),
            format!("prefix:{TEMPORARY}"),
            format!("sha256:{TEMPORARY}"),
            validation(TEMPORARY, 20),
            format!("commit:{TEMPORARY}"),
            validation(COMMITTED, 20),
            format!("sync:{COMMITTED}"),
            "read-bcb".to_owned(),
            "recovery-exists".to_owned(),
            validation(COMMITTED, 20),
            format!("commit-bcb:{COMMITTED}:20"),
            "read-bcb".to_owned(),
            "reboot".to_owned(),
            "unlock".to_owned(),
        ]
    );
}

#[test]
fn recovery_install_revalidates_immediately_before_update_engine() {
    let platform = FakeFirmwarePlatform::default();
    let inspect = platform.clone();
    let service = OtaService::new(platform);

    service
        .install(
            Path::new("/incoming/recovery.fw"),
            EXPECTED_DIGEST,
            InstallMode::IncludeRecovery,
            false,
        )
        .unwrap();

    let calls = inspect.calls();
    let update = calls
        .iter()
        .position(|call| call.starts_with("update-engine:"))
        .unwrap();
    assert_eq!(calls[update - 1], validation(COMMITTED, 20));
    assert!(!calls.iter().any(|call| call == "recovery-exists"));
}

#[test]
fn pending_command_refuses_both_staging_backends() {
    for mode in [InstallMode::RecoveryFree, InstallMode::IncludeRecovery] {
        let platform = FakeFirmwarePlatform::default();
        platform.bcb.borrow_mut()[0] = 1;
        let inspect = platform.clone();
        let service = OtaService::new(platform);

        let error = service
            .install(
                Path::new("/incoming/upgrade.fw"),
                EXPECTED_DIGEST,
                mode,
                false,
            )
            .unwrap_err();

        assert!(error.to_string().contains("pending bootloader command"));
        assert!(!inspect
            .calls()
            .iter()
            .any(|call| call.starts_with("commit-bcb:") || call.starts_with("update-engine:")));
    }
}

#[test]
fn cli_keeps_deployment_path_out_of_arguments() {
    assert_eq!(
        parse_ota_cli([
            "download",
            "https://example.invalid/upgrade.fw",
            EXPECTED_DIGEST,
        ])
        .unwrap(),
        OtaCommand::Download {
            source: "https://example.invalid/upgrade.fw".to_owned(),
            expected: EXPECTED_DIGEST.to_owned(),
        }
    );
    assert!(parse_ota_cli([
        "download",
        "https://example.invalid/upgrade.fw",
        EXPECTED_DIGEST,
        "/arbitrary/deployment.fw",
    ])
    .is_err());
}

#[test]
fn application_verify_uses_an_opened_firmware_identity() {
    let platform = FakeFirmwarePlatform::default();
    let inspect = platform.clone();
    let service = OtaService::new(platform);

    service
        .verify(Path::new("/incoming/upgrade.fw"), EXPECTED_DIGEST)
        .unwrap();

    assert_eq!(
        inspect.calls(),
        [
            "open:/incoming/upgrade.fw".to_owned(),
            validation("/incoming/upgrade.fw", 10),
            "prefix:/incoming/upgrade.fw".to_owned(),
            "sha256:/incoming/upgrade.fw".to_owned(),
            validation("/incoming/upgrade.fw", 10),
        ]
    );
}

#[test]
fn digest_accepts_uppercase_and_rejects_non_hex_or_wrong_length() {
    assert_eq!(
        Sha256Digest::parse(&"A".repeat(64)).unwrap().to_hex(),
        EXPECTED_DIGEST
    );
    assert_eq!(
        Sha256Digest::parse("not-a-digest").unwrap_err(),
        OtaDomainError::InvalidDigest
    );
}

#[test]
fn builds_rockchip_recovery_free_bcb() {
    let message = BootloaderMessage::for_update_package(COMMITTED).unwrap();
    assert_eq!(&message.as_bytes()[..13], b"boot-recovery");
    let recovery = format!("recovery\n--update_package={COMMITTED}\n");
    assert_eq!(
        &message.as_bytes()[64..64 + recovery.len()],
        recovery.as_bytes()
    );
    assert_eq!(
        &message.as_bytes()[BCB_NEED_UPDATE_OFFSET..BCB_NEED_UPDATE_OFFSET + 4],
        &RECOVERY_UPDATE_MASK.to_le_bytes()
    );
}

#[test]
fn exact_bcb_verification_rejects_trailing_or_single_byte_changes() {
    let expected = BootloaderMessage::for_update_package(COMMITTED).unwrap();
    verify_bcb_exact(&expected, expected.as_bytes()).unwrap();

    let mut changed = expected.as_bytes().to_vec();
    changed[BCB_SIZE - 1] = 1;
    assert_eq!(
        verify_bcb_exact(&expected, &changed).unwrap_err(),
        OtaDomainError::BcbVerificationFailed
    );

    changed.push(0);
    assert!(matches!(
        verify_bcb_exact(&expected, &changed),
        Err(OtaDomainError::InvalidBcbSize { .. })
    ));
}

#[test]
fn bcb_rejects_control_characters_long_paths_and_pending_commands() {
    assert!(BootloaderMessage::for_update_package("/userdata/bad\npath.fw").is_err());
    let path = format!("/userdata/{}.fw", "a".repeat(760));
    assert!(BootloaderMessage::for_update_package(&path).is_err());

    let mut current = vec![0_u8; BCB_SIZE];
    ensure_no_pending_command(&current).unwrap();
    current[31] = 1;
    assert_eq!(
        ensure_no_pending_command(&current).unwrap_err(),
        OtaDomainError::PendingBootloaderCommand
    );
}

#[test]
fn filesystem_adapter_reads_bcb_at_rockchip_offset() {
    let root = std::env::temp_dir().join(format!("hyz-router-ota-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let misc_path = root.join("misc");
    let misc = File::create(&misc_path).unwrap();
    misc.set_len(BCB_OFFSET + BCB_SIZE as u64).unwrap();
    drop(misc);
    let recovery_path = root.join("recovery");
    File::create(&recovery_path).unwrap();

    let expected = BootloaderMessage::for_update_package(COMMITTED).unwrap();
    let mut misc = OpenOptions::new().write(true).open(&misc_path).unwrap();
    misc.seek(SeekFrom::Start(BCB_OFFSET)).unwrap();
    misc.write_all(expected.as_bytes()).unwrap();
    misc.sync_all().unwrap();

    let adapter = FirmwareAdapter::new(FirmwareAdapterConfig {
        update_engine: root.join("updateEngine"),
        reboot: root.join("reboot"),
        misc_partition: misc_path,
        recovery_partition: recovery_path,
        staging_directory: root.join("staging"),
        mutation_lock: root.join("ota-lock"),
    });
    let actual = adapter.read_bcb().unwrap();
    verify_bcb_exact(&expected, &actual).unwrap();

    fs::remove_dir_all(root).unwrap();
}
