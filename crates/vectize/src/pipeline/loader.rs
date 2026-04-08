//! Image loading from files and byte slices.
//!
//! Supports PNG, JPEG/JPG, WebP, and other common bitmap formats via the `image` crate.

use std::path::Path;

use image::{DynamicImage, ImageError, ImageFormat, ImageReader};

use crate::error::{Result, VectizeError};

/// File extensions recognized by directory-scanning helpers such as the CLI batch mode.
pub const SUPPORTED_BITMAP_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "bmp", "gif", "ico", "pnm", "pbm", "pgm", "ppm", "pam", "tga",
    "tif", "tiff",
];

/// Human-readable summary of the bitmap formats supported by the native loader.
pub const SUPPORTED_BITMAP_FORMATS_SUMMARY: &str =
    "PNG, JPEG/JPG, WebP, BMP, GIF, ICO, PNM, TGA, and TIFF";

/// Return `true` when the path uses a bitmap extension recognized by the loader.
///
/// This helper is mainly intended for directory scanning and CLI pre-filtering.
pub fn is_supported_bitmap_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| SUPPORTED_BITMAP_EXTENSIONS.contains(&extension.as_str()))
}

/// Load an image from a file path.
///
/// The decoder sniffs the actual file contents, so supported images can still be
/// loaded even when the file extension is missing or non-standard.
pub fn load_from_file(path: &Path) -> Result<DynamicImage> {
    let img = ImageReader::open(path)
        .map_err(VectizeError::Io)?
        .with_guessed_format()
        .map_err(VectizeError::Io)?
        .decode()
        .map_err(|error| unsupported_format_or_decode_error(error, Some(path)))?;

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
        .map_err(|error| unsupported_format_or_decode_error(error, None))?;
    Ok(img)
}

fn unsupported_format_or_decode_error(error: ImageError, path: Option<&Path>) -> VectizeError {
    match error {
        ImageError::Unsupported(_) => {
            let detected = path
                .and_then(|path| ImageFormat::from_path(path).ok())
                .map(|format| format!("Detected format hint: {format:?}. "))
                .unwrap_or_default();
            let source = path
                .map(|path| format!("File '{}'", path.display()))
                .unwrap_or_else(|| "Input bytes".to_string());

            VectizeError::UnsupportedFormat(format!(
                "{source} is not a supported bitmap image. {detected}Supported inputs include {SUPPORTED_BITMAP_FORMATS_SUMMARY}."
            ))
        }
        other => VectizeError::ImageDecode(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    fn unique_temp_path(extension: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("vectizeit_loader_{unique}.{extension}"))
    }

    #[test]
    fn load_from_file_accepts_content_sniffed_images_and_rejects_non_images() {
        assert!(is_supported_bitmap_path(Path::new("image.bmp")));
        assert!(is_supported_bitmap_path(Path::new("image.TIFF")));
        assert!(!is_supported_bitmap_path(Path::new("image.svg")));

        let sniffed_path = unique_temp_path("image");
        let invalid_path = unique_temp_path("txt");

        let png = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
        let mut buffer = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(png)
            .write_to(&mut buffer, ImageFormat::Png)
            .unwrap();

        std::fs::write(&sniffed_path, buffer.into_inner()).unwrap();
        std::fs::write(&invalid_path, b"not an image").unwrap();

        let sniffed = load_from_file(&sniffed_path).unwrap();
        assert_eq!(sniffed.width(), 2);
        assert_eq!(sniffed.height(), 2);

        let invalid = load_from_file(&invalid_path).unwrap_err();
        assert!(matches!(invalid, VectizeError::UnsupportedFormat(_)));
        assert!(invalid
            .to_string()
            .contains(SUPPORTED_BITMAP_FORMATS_SUMMARY));

        let _ = std::fs::remove_file(sniffed_path);
        let _ = std::fs::remove_file(invalid_path);
    }
}
