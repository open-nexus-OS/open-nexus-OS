// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::decode::DecodedImage;
use crate::error::{ImageError, ImageResult};

/// Scaling filter type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleFilter {
    Nearest,
    Bilinear,
}

/// Scale a decoded image to new dimensions.
///
/// Uses bilinear filtering for downscaling (by default) and supports both
/// nearest-neighbor and bilinear for upscaling.
pub fn scale_image(
    image: &DecodedImage,
    target_width: u32,
    target_height: u32,
    filter: ScaleFilter,
) -> ImageResult<DecodedImage> {
    if target_width == 0 || target_height == 0 {
        return Err(ImageError::InvalidScaleTarget { target_width, target_height });
    }

    // Same size — return clone
    if target_width == image.width && target_height == image.height {
        return Ok(image.clone());
    }

    let sw = image.width as f32;
    let sh = image.height as f32;
    let dw = target_width as f32;
    let dh = target_height as f32;

    let mut out = vec![0u8; (target_width as usize) * (target_height as usize) * 4];

    match filter {
        ScaleFilter::Nearest => {
            for y in 0..target_height {
                let sy = ((y as f32 + 0.5) * sh / dh) as u32;
                let sy = sy.min(image.height.saturating_sub(1));
                for x in 0..target_width {
                    let sx = ((x as f32 + 0.5) * sw / dw) as u32;
                    let sx = sx.min(image.width.saturating_sub(1));
                    let si = ((sy * image.width + sx) * 4) as usize;
                    let di = ((y * target_width + x) * 4) as usize;
                    out[di..di + 4].copy_from_slice(&image.data[si..si + 4]);
                }
            }
        }
        ScaleFilter::Bilinear => {
            for y in 0..target_height {
                let sy = (y as f32 + 0.5) * sh / dh - 0.5;
                let sy0 = (sy.floor() as i32).max(0) as u32;
                let sy1 = (sy0 + 1).min(image.height.saturating_sub(1));
                let fy = sy - sy.floor();

                for x in 0..target_width {
                    let sx = (x as f32 + 0.5) * sw / dw - 0.5;
                    let sx0 = (sx.floor() as i32).max(0) as u32;
                    let sx1 = (sx0 + 1).min(image.width.saturating_sub(1));
                    let fx = sx - sx.floor();

                    let i00 = ((sy0 * image.width + sx0) * 4) as usize;
                    let i10 = ((sy0 * image.width + sx1) * 4) as usize;
                    let i01 = ((sy1 * image.width + sx0) * 4) as usize;
                    let i11 = ((sy1 * image.width + sx1) * 4) as usize;

                    let di = ((y * target_width + x) * 4) as usize;
                    for c in 0..4 {
                        let v00 = image.data[i00 + c] as f32;
                        let v10 = image.data[i10 + c] as f32;
                        let v01 = image.data[i01 + c] as f32;
                        let v11 = image.data[i11 + c] as f32;
                        let v = v00 * (1.0 - fx) * (1.0 - fy)
                            + v10 * fx * (1.0 - fy)
                            + v01 * (1.0 - fx) * fy
                            + v11 * fx * fy;
                        out[di + c] = v.round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }

    Ok(DecodedImage { width: target_width, height: target_height, data: out })
}
