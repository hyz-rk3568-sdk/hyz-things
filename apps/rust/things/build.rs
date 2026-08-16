use std::{
    env,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use tar::{Archive, Builder, Header};

const MAX_ARCHIVE_BYTES: u64 = 32 * 1024 * 1024;
const PLACEHOLDER: &[u8] = br#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>hyz things</title></head><body><main><h1>hyz things</h1><p>Frontend bundle is not installed.</p></main></body></html>"#;

fn main() -> io::Result<()> {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let default_archive = manifest.join("../../../target/frontend-bundle/hyz-things-frontend.tar");
    let source = env::var_os("HYZ_THINGS_FRONTEND_ARCHIVE")
        .map(PathBuf::from)
        .unwrap_or(default_archive);
    let output =
        PathBuf::from(env::var_os("OUT_DIR").expect("output directory")).join("frontend.tar");

    println!("cargo:rerun-if-env-changed=HYZ_THINGS_FRONTEND_ARCHIVE");
    println!("cargo:rerun-if-changed={}", source.display());
    if source.is_file() {
        validate_archive(&source)?;
        fs::copy(&source, &output)?;
    } else {
        write_placeholder(&output)?;
        println!("cargo:warning=hyz-things frontend bundle absent; embedding safe placeholder");
    }
    Ok(())
}

fn validate_archive(path: &Path) -> io::Result<()> {
    if fs::metadata(path)?.len() > MAX_ARCHIVE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frontend archive is too large",
        ));
    }
    let mut has_index = false;
    for entry in Archive::new(File::open(path)?).entries()? {
        let entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let archive_path = entry.path()?.to_string_lossy().replace('\\', "/");
        let archive_path = archive_path.trim_start_matches("./");
        if !safe_archive_path(archive_path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frontend archive contains an unsafe path",
            ));
        }
        has_index |= archive_path == "index.html";
    }
    if has_index {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frontend archive has no index.html",
        ))
    }
}

fn safe_archive_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn write_placeholder(path: &Path) -> io::Result<()> {
    let mut archive = Builder::new(File::create(path)?);
    let mut header = Header::new_gnu();
    header.set_size(PLACEHOLDER.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    archive.append_data(&mut header, "index.html", PLACEHOLDER)?;
    archive.finish()
}
