// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::error::{ImageError, ImageResult};
use crate::limits::{DECOMPRESSION_BOMB_RATIO, MAX_DECODE_PIXELS, MAX_IMAGE_DIMENSION};

/// Decoded RGBA8 image.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>, // RGBA8, row-major
}

/// Recognized image formats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

/// Decode an image from raw bytes. Format is auto-detected from the header.
/// Bounds are enforced: dimensions must be ≤ MAX_IMAGE_DIMENSION each,
/// total pixels ≤ MAX_DECODE_PIXELS, and a decompression bomb check is applied.
pub fn decode_image(bytes: &[u8]) -> ImageResult<DecodedImage> {
    if bytes.is_empty() {
        return Err(ImageError::EmptyInput);
    }

    let format = detect_format(bytes)?;

    let image = match format {
        ImageFormat::Png => decode_png(bytes)?,
        ImageFormat::Jpeg => decode_jpeg(bytes)?,
    };

    // Dimension limits
    if image.width > MAX_IMAGE_DIMENSION || image.height > MAX_IMAGE_DIMENSION {
        return Err(ImageError::DimensionTooLarge {
            width: image.width,
            height: image.height,
            pixels: image.width as u64 * image.height as u64,
            max_pixels: MAX_DECODE_PIXELS,
        });
    }

    let pixels = image.width as u64 * image.height as u64;
    if pixels > MAX_DECODE_PIXELS {
        return Err(ImageError::DimensionTooLarge {
            width: image.width,
            height: image.height,
            pixels,
            max_pixels: MAX_DECODE_PIXELS,
        });
    }

    // Decompression bomb check
    let compressed_bytes = bytes.len() as u64;
    if compressed_bytes > 0 && pixels > compressed_bytes * DECOMPRESSION_BOMB_RATIO {
        return Err(ImageError::DecompressionBomb {
            compressed_bytes: bytes.len(),
            output_pixels: pixels,
        });
    }

    Ok(image)
}

fn detect_format(bytes: &[u8]) -> ImageResult<ImageFormat> {
    if bytes.len() < 4 {
        return Err(ImageError::UnknownFormat);
    }

    // PNG: 89 50 4E 47
    if bytes[0] == 0x89 && bytes[1] == 0x50 && bytes[2] == 0x4E && bytes[3] == 0x47 {
        return Ok(ImageFormat::Png);
    }

    // JPEG: FF D8 FF
    if bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Ok(ImageFormat::Jpeg);
    }

    Err(ImageError::UnknownFormat)
}

fn decode_png(bytes: &[u8]) -> ImageResult<DecodedImage> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().map_err(|e| ImageError::PngDecode(e.to_string()))?;

    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| ImageError::PngDecode(e.to_string()))?;

    let width = info.width;
    let height = info.height;

    // Convert to RGBA8
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for chunk in buf.chunks(3) {
                rgba.push(chunk[0]);
                rgba.push(chunk[1]);
                rgba.push(chunk[2]);
                rgba.push(255);
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for &g in &buf {
                rgba.push(g);
                rgba.push(g);
                rgba.push(g);
                rgba.push(255);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for chunk in buf.chunks(2) {
                rgba.push(chunk[0]);
                rgba.push(chunk[0]);
                rgba.push(chunk[0]);
                rgba.push(chunk[1]);
            }
            rgba
        }
        _ => {
            return Err(ImageError::PngDecode("unsupported color type".to_string()));
        }
    };

    Ok(DecodedImage { width, height, data: rgba })
}

fn decode_jpeg(bytes: &[u8]) -> ImageResult<DecodedImage> {
    use jpeg_decoder::Decoder;

    let mut decoder = Decoder::new(bytes);
    let pixels = decoder.decode().map_err(|e| ImageError::JpegDecode(e.to_string()))?;

    let metadata = decoder
        .info()
        .ok_or_else(|| ImageError::JpegDecode("unable to read JPEG metadata".to_string()))?;
    let width = metadata.width as u32;
    let height = metadata.height as u32;

    // JPEG decoder returns RGB, convert to RGBA
    let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for chunk in pixels.chunks(3) {
        rgba.push(chunk[0]);
        rgba.push(chunk[1]);
        rgba.push(chunk[2]);
        rgba.push(255);
    }

    // Handle grayscale JPEGs
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        let gray_count = pixels.len();
        rgba.clear();
        rgba.reserve(gray_count * 4);
        for i in 0..gray_count {
            let g = pixels[i];
            rgba.push(g);
            rgba.push(g);
            rgba.push(g);
            rgba.push(255);
        }
    }

    Ok(DecodedImage { width, height, data: rgba })
}
