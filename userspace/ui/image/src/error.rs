// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

/// Result type alias for image operations.
pub type ImageResult<T> = Result<T, ImageError>;

/// Errors that can occur during image decode and scale operations.
#[derive(Debug)]
pub enum ImageError {
    /// Input data is empty or too small to be a valid image.
    EmptyInput,

    /// Unrecognized image format.
    UnknownFormat,

    /// PNG decode error.
    PngDecode(String),

    /// JPEG decode error.
    JpegDecode(String),

    /// Image dimensions exceed the maximum allowed.
    DimensionTooLarge { width: u32, height: u32, pixels: u64, max_pixels: u64 },

    /// Decompression bomb detected (small compressed size, huge output).
    DecompressionBomb { compressed_bytes: usize, output_pixels: u64 },

    /// Scale operation failed (e.g. zero target dimensions).
    InvalidScaleTarget { target_width: u32, target_height: u32 },
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::EmptyInput => write!(f, "image input is empty"),
            ImageError::UnknownFormat => write!(f, "unknown image format"),
            ImageError::PngDecode(msg) => write!(f, "PNG decode error: {msg}"),
            ImageError::JpegDecode(msg) => write!(f, "JPEG decode error: {msg}"),
            ImageError::DimensionTooLarge { width, height, pixels, max_pixels } => {
                write!(
                    f,
                    "image dimensions {width}x{height} ({pixels} px) exceed limit ({max_pixels} px)"
                )
            }
            ImageError::DecompressionBomb { compressed_bytes, output_pixels } => {
                write!(f, "decompression bomb: {compressed_bytes} bytes → {output_pixels} pixels")
            }
            ImageError::InvalidScaleTarget { target_width, target_height } => {
                write!(f, "invalid scale target: {target_width}x{target_height}")
            }
        }
    }
}

impl std::error::Error for ImageError {}
