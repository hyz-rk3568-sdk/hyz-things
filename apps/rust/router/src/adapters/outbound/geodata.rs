use crate::application::ports::PlatformError;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const O_NOFOLLOW: i32 = 0o400000;
const PACKAGED_GEODATA_DIR: &str = "/usr/share/hyz-router/mihomo";

#[derive(Clone, Copy)]
struct GeoAsset {
    packaged_name: &'static str,
    runtime_name: &'static str,
    size: u64,
    sha256: &'static str,
}

const GEO_ASSETS: [GeoAsset; 2] = [
    GeoAsset {
        packaged_name: "geosite.dat",
        runtime_name: "GeoSite.dat",
        size: 4_244_097,
        sha256: "83e5023cfc134700fd373d800880c62c9cb1e8b97aef59052d7779d817c27be0",
    },
    GeoAsset {
        packaged_name: "country.mmdb",
        runtime_name: "Country.mmdb",
        size: 7_824_943,
        sha256: "4790a1479e63c8d3b67af7b47f6cb0b96a8d05a6d01ed568af8385c1d1176c8e",
    },
];

pub(crate) fn ensure_mihomo_geodata(data_dir: &str) -> Result<(), PlatformError> {
    let data_dir = Path::new(data_dir);
    for asset in GEO_ASSETS {
        let packaged = Path::new(PACKAGED_GEODATA_DIR).join(asset.packaged_name);
        verify_asset(&packaged, asset, false)?;

        let runtime = data_dir.join(asset.runtime_name);
        if verify_asset(&runtime, asset, true).is_ok() {
            continue;
        }
        install_asset(&packaged, &runtime, asset)?;
        verify_asset(&runtime, asset, true)?;
    }
    Ok(())
}

fn install_asset(source: &Path, destination: &Path, asset: GeoAsset) -> Result<(), PlatformError> {
    let parent = destination.parent().ok_or_else(|| {
        PlatformError::InvalidState("Mihomo GeoData destination has no parent".to_owned())
    })?;
    let temporary = unique_temporary(destination);
    let mut source = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(source)
        .map_err(|error| PlatformError::Io(format!("open packaged GeoData: {error}")))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW)
        .open(&temporary)
        .map_err(|error| PlatformError::Io(format!("create GeoData staging file: {error}")))?;
    let result = (|| {
        std::io::copy(&mut source, &mut output)
            .map_err(|error| PlatformError::Io(format!("copy packaged GeoData: {error}")))?;
        output
            .sync_all()
            .map_err(|error| PlatformError::Io(format!("sync GeoData staging file: {error}")))?;
        verify_asset(&temporary, asset, true)?;
        fs::rename(&temporary, destination)
            .map_err(|error| PlatformError::Io(format!("commit GeoData runtime copy: {error}")))?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PlatformError::Io(format!("sync Mihomo data directory: {error}")))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn verify_asset(path: &Path, asset: GeoAsset, private: bool) -> Result<(), PlatformError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        PlatformError::InvalidState(format!("required Mihomo GeoData {} is unavailable: {error}", path.display()))
    })?;
    if !metadata.file_type().is_file()
        || metadata.uid() != 0
        || (private && metadata.mode() & 0o077 != 0)
        || metadata.len() != asset.size
    {
        return Err(PlatformError::InvalidState(format!(
            "required Mihomo GeoData {} has invalid metadata",
            path.display()
        )));
    }
    let digest = sha256_file(path)?;
    if digest != asset.sha256 {
        return Err(PlatformError::InvalidState(format!(
            "required Mihomo GeoData {} failed SHA-256 verification",
            path.display()
        )));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, PlatformError> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|error| PlatformError::Io(format!("open GeoData for hashing: {error}")))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| PlatformError::Io(format!("read GeoData for hashing: {error}")))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn unique_temporary(destination: &Path) -> PathBuf {
    destination.with_extension(format!("hyz-{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_assets_match_expected_mihomo_runtime_names() {
        assert_eq!(GEO_ASSETS[0].runtime_name, "GeoSite.dat");
        assert_eq!(GEO_ASSETS[1].runtime_name, "Country.mmdb");
        assert_eq!(GEO_ASSETS[0].sha256.len(), 64);
        assert_eq!(GEO_ASSETS[1].sha256.len(), 64);
    }
}
