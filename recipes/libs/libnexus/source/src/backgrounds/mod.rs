use orbimage::{Image, ResizeType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundMode {
    /// Do not resize the image, just center it
    Center,
    /// Resize the image to the display size
    Fill,
    /// Resize the image - keeping its aspect ratio, and fit it to the display with blank space
    Scale,
    /// Resize the image - keeping its aspect ratio, and crop to remove all blank space
    Zoom,
}

/// Scale and crop an image according to [`BackgroundMode`]. Returns `None` if the
/// requested target size is zero in either dimension or if the resize fails.
pub fn scale_for_mode(image: &Image, mode: BackgroundMode, target: (u32, u32)) -> Option<Image> {
    let (target_w, target_h) = target;
    if target_w == 0 || target_h == 0 {
        return None;
    }

    let (scaled_w, scaled_h) = match mode {
        BackgroundMode::Center => (image.width(), image.height()),
        BackgroundMode::Fill => (target_w, target_h),
        BackgroundMode::Scale => {
            let d_w = target_w as f64;
            let d_h = target_h as f64;
            let i_w = image.width() as f64;
            let i_h = image.height() as f64;

            let scale = if d_w / d_h > i_w / i_h { d_h / i_h } else { d_w / i_w };

            (
                ((i_w * scale).round() as u32).max(1),
                ((i_h * scale).round() as u32).max(1),
            )
        }
        BackgroundMode::Zoom => {
            let d_w = target_w as f64;
            let d_h = target_h as f64;
            let i_w = image.width() as f64;
            let i_h = image.height() as f64;

            let scale = if d_w / d_h < i_w / i_h { d_h / i_h } else { d_w / i_w };

            (
                ((i_w * scale).round() as u32).max(1),
                ((i_h * scale).round() as u32).max(1),
            )
        }
    };

    let scaled = if scaled_w == image.width() && scaled_h == image.height() {
        image.clone()
    } else {
        match image.clone().resize(scaled_w, scaled_h, ResizeType::Lanczos3) {
            Ok(img) => img,
            Err(_) => return None,
        }
    };

    let crop_x = if scaled_w > target_w {
        (scaled_w - target_w) / 2
    } else {
        0
    };
    let crop_y = if scaled_h > target_h {
        (scaled_h - target_h) / 2
    } else {
        0
    };

    let crop_w = scaled_w.min(target_w);
    let crop_h = scaled_h.min(target_h);

    Some(scaled.roi(crop_x, crop_y, crop_w, crop_h))
}
