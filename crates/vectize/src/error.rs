//! Error types for the vectize library.

use thiserror::Error;

/// The main error type for all vectize operations.
#[derive(Debug, Error)]
pub enum VectizeError {
    /// An I/O error occurred while reading or writing a file.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The image could not be decoded or is in an unsupported format.
    #[error("Image decoding error: {0}")]
    ImageDecode(#[from] image::ImageError),

    /// The provided configuration is invalid.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// An error occurred during the vectorization pipeline.
    #[error("Pipeline error: {0}")]
    Pipeline(String),

    /// The input format is not supported.
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
}

/// A specialized `Result` type for vectize operations.
pub type Result<T> = std::result::Result<T, VectizeError>;
