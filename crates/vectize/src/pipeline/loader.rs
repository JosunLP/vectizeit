//! Image loading from files and byte slices.
//!
//! Supports PNG, JPEG/JPG, and WebP via the `image` crate.

use std::path::Path;

use image::{DynamicImage, ImageReader};

use crate::error::{Result, VectizeError};

/// Load an image from a file path.
///
/// The format is inferred from the file extension.
/// Supported formats: PNG, JPEG/JPG, WebP.
pub fn load_from_file(path: &Path) -> Result<DynamicImage> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" => {}
        other => {
            return Err(VectizeError::UnsupportedFormat(format!(
                "File extension '.{other}' is not supported. Use PNG, JPEG, or WebP."
            )));
        }
    }

    let img = ImageReader::open(path)
        .map_err(VectizeError::Io)?
        .with_guessed_format()
        .map_err(VectizeError::Io)?
        .decode()
        .map_err(VectizeError::ImageDecode)?;

    Ok(img)
}

/// Load an image from raw bytes.
///
/// The format is inferred from the byte magic header.
pub fn load_from_bytes(bytes: &[u8]) -> Result<DynamicImage> {
    let cursor = std::io::Cursor::new(bytes);
    let img = ImageReader::new(cursor)
        .with_guessed_format()
        .map_err(VectizeError::Io)?
        .decode()
        .map_err(VectizeError::ImageDecode)?;
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_extension_is_rejected() {
        let path = Path::new("image.bmp");
        let err = load_from_file(path).unwrap_err();
        assert!(matches!(err, VectizeError::UnsupportedFormat(_)));
    }
}
