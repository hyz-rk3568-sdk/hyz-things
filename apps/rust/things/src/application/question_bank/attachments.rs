//! Pure validation for temporary AI message images. No filesystem or HTTP.
use super::BankError;

pub const MAX_IMAGES_PER_MESSAGE: usize = 3;
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGE_SIDE: u32 = 12_000;
pub const MAX_IMAGE_PIXELS: u64 = 40_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidatedImage {
    pub media_type: &'static str,
    pub extension: &'static str,
    pub width: u32,
    pub height: u32,
}

fn dimensions_ok(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_IMAGE_SIDE
        && height <= MAX_IMAGE_SIDE
        && u64::from(width) * u64::from(height) <= MAX_IMAGE_PIXELS
}

fn png(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 33 || bytes[..8] != SIGNATURE {
        return None;
    }
    let chunk = u32::from_be_bytes(bytes[8..12].try_into().ok()?) as usize;
    if chunk != 13 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    let valid_shape = bytes[24] > 0
        && matches!(bytes[25], 0 | 2 | 3 | 4 | 6)
        && bytes[26] == 0
        && bytes[27] == 0
        && matches!(bytes[28], 0 | 1);
    valid_shape.then_some((width, height))
}

fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }
    let mut offset = 2;
    while offset + 4 <= bytes.len() {
        if bytes[offset] != 0xff {
            offset += 1;
            continue;
        }
        let marker = bytes[offset + 1];
        if marker == 0xff {
            offset += 1;
            continue;
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            offset += 2;
            continue;
        }
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        let length = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
        if length < 2 || offset + 2 + length > bytes.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if length < 7 {
                return None;
            }
            let height = u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[offset + 7], bytes[offset + 8]]) as u32;
            return Some((width, height));
        }
        offset += 2 + length;
    }
    None
}

/// Validate the claimed MIME type, file signature and pixel limits. The
/// extension is server-chosen so a caller cannot affect the receiver's path.
pub fn validate_image(bytes: &[u8], media_type: &str) -> Result<ValidatedImage, BankError> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(BankError::Invalid);
    }
    let media_type = media_type.split(';').next().unwrap_or("").trim();
    let (media_type, extension, dimensions) = match media_type {
        "image/png" => ("image/png", "png", png(bytes)),
        "image/jpeg" => ("image/jpeg", "jpg", jpeg(bytes)),
        _ => return Err(BankError::Invalid),
    };
    let (width, height) = dimensions.ok_or(BankError::Invalid)?;
    if !dimensions_ok(width, height) {
        return Err(BankError::Invalid);
    }
    Ok(ValidatedImage {
        media_type,
        extension,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8];
        bytes.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        bytes.extend_from_slice(&[0xff, 0xd9]);
        bytes
    }

    #[test]
    fn accepts_png_and_jpeg_with_matching_signatures() {
        assert_eq!(
            validate_image(&png(640, 480), "image/png").unwrap(),
            ValidatedImage {
                media_type: "image/png",
                extension: "png",
                width: 640,
                height: 480,
            }
        );
        let image = validate_image(&jpeg(800, 600), "image/jpeg; charset=binary").unwrap();
        assert_eq!(image.extension, "jpg");
        assert_eq!((image.width, image.height), (800, 600));
    }

    #[test]
    fn rejects_mismatched_or_unknown_types() {
        assert_eq!(
            validate_image(&png(10, 10), "image/jpeg"),
            Err(BankError::Invalid)
        );
        assert_eq!(
            validate_image(&jpeg(10, 10), "image/png"),
            Err(BankError::Invalid)
        );
        assert_eq!(
            validate_image(b"not an image", "image/png"),
            Err(BankError::Invalid)
        );
        assert_eq!(
            validate_image(b"GIF89a", "image/gif"),
            Err(BankError::Invalid)
        );
    }

    #[test]
    fn rejects_oversized_or_empty_images() {
        assert_eq!(validate_image(&[], "image/png"), Err(BankError::Invalid));
        assert_eq!(
            validate_image(&png(MAX_IMAGE_SIDE + 1, 10), "image/png"),
            Err(BankError::Invalid)
        );
        assert_eq!(
            validate_image(&png(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE), "image/png"),
            Err(BankError::Invalid)
        );
        assert_eq!(
            validate_image(&vec![0u8; MAX_IMAGE_BYTES + 1], "image/png"),
            Err(BankError::Invalid)
        );
    }
}
