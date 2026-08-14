use crate::{application::ports::MediaError, domain::FIXED_CAMERA_DEVICE};
use std::{fs, os::unix::fs::FileTypeExt, path::Path};

pub fn probe_fixed_camera_device() -> Result<(), MediaError> {
    let path = Path::new(FIXED_CAMERA_DEVICE);
    let symlink = fs::symlink_metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => MediaError::CameraNotFound,
        std::io::ErrorKind::PermissionDenied => MediaError::UnknownOwnership,
        _ => MediaError::PipelineFailed,
    })?;
    if symlink.file_type().is_symlink() || !symlink.file_type().is_char_device() {
        return Err(MediaError::UnknownOwnership);
    }
    Ok(())
}
