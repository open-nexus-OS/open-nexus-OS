// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Frame drawing primitives for the TASK-0054 host renderer.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use crate::damage::Damage;
use crate::error::{RenderError, RenderResult};
use crate::font::FixtureFont;
use crate::frame::Frame;
use crate::geometry::{Point, Rect};
use crate::image::Image;
use crate::limits::{BYTES_PER_PIXEL, MAX_GLYPHS};
use crate::math::rounded_rect_covers;
use crate::pixel::PixelBgra;

impl Frame {
    pub fn clear(&mut self, color: PixelBgra, damage: &mut Damage) -> RenderResult<()> {
        let row_len = usize::try_from(self.width.get() * BYTES_PER_PIXEL)
            .map_err(|_| RenderError::ArithmeticOverflow)?;
        let stride =
            usize::try_from(self.stride.get()).map_err(|_| RenderError::ArithmeticOverflow)?;
        let height =
            usize::try_from(self.height.get()).map_err(|_| RenderError::ArithmeticOverflow)?;
        for y in 0..height {
            let row = y.checked_mul(stride).ok_or(RenderError::ArithmeticOverflow)?;
            for pixel in self.buffer[row..row + row_len].chunks_exact_mut(4) {
                pixel.copy_from_slice(&color.bytes());
            }
        }
        damage.add(Rect::new(0, 0, self.width.get(), self.height.get())?)
    }

    pub fn draw_rect(
        &mut self,
        rect: Rect,
        color: PixelBgra,
        damage: &mut Damage,
    ) -> RenderResult<()> {
        let Some(clipped) = rect.clip_to(self.bounds_rect()?) else {
            return Ok(());
        };
        self.fill_clipped_rect(clipped, color)?;
        damage.add(clipped)
    }

    pub fn draw_rounded_rect(
        &mut self,
        rect: Rect,
        radius: u32,
        color: PixelBgra,
        damage: &mut Damage,
    ) -> RenderResult<()> {
        let Some(clipped) = rect.clip_to(self.bounds_rect()?) else {
            return Ok(());
        };
        let radius = radius.min(rect.width / 2).min(rect.height / 2);
        for y in clipped.y..i32::try_from(clipped.bottom()).map_err(|_| RenderError::InvalidRect)? {
            for x in
                clipped.x..i32::try_from(clipped.right()).map_err(|_| RenderError::InvalidRect)?
            {
                if rounded_rect_covers(rect, radius, x, y)? {
                    self.set_pixel_checked(x, y, color)?;
                }
            }
        }
        damage.add(clipped)
    }

    pub fn blit(&mut self, dst: Point, source: &Image, damage: &mut Damage) -> RenderResult<()> {
        let dst_rect = Rect::new(dst.x, dst.y, source.width.get(), source.height.get())?;
        let Some(clipped) = dst_rect.clip_to(self.bounds_rect()?) else {
            return Ok(());
        };
        for y in 0..clipped.height {
            for x in 0..clipped.width {
                let dst_x = clipped.x + i32::try_from(x).map_err(|_| RenderError::InvalidRect)?;
                let dst_y = clipped.y + i32::try_from(y).map_err(|_| RenderError::InvalidRect)?;
                let src_x = u32::try_from(dst_x - dst.x).map_err(|_| RenderError::InvalidRect)?;
                let src_y = u32::try_from(dst_y - dst.y).map_err(|_| RenderError::InvalidRect)?;
                let color = source.pixel(src_x, src_y)?;
                self.set_pixel_checked(dst_x, dst_y, color)?;
            }
        }
        damage.add(clipped)
    }

    pub fn draw_text(
        &mut self,
        position: Point,
        text: &str,
        font: &FixtureFont,
        color: PixelBgra,
        damage: &mut Damage,
    ) -> RenderResult<()> {
        let glyph_count = text.chars().count();
        if glyph_count == 0 {
            return Ok(());
        }
        if glyph_count > MAX_GLYPHS {
            return Err(RenderError::GlyphRunTooLarge);
        }
        for ch in text.chars() {
            if font.glyph(ch).is_none() {
                return Err(RenderError::Unsupported);
            }
        }

        let advance = font.width.checked_add(1).ok_or(RenderError::ArithmeticOverflow)?;
        let total_width = u32::try_from(glyph_count)
            .map_err(|_| RenderError::GlyphRunTooLarge)?
            .checked_mul(advance)
            .and_then(|value| value.checked_sub(1))
            .ok_or(RenderError::ArithmeticOverflow)?;
        let bounds = Rect::new(position.x, position.y, total_width, font.height)?;

        for (index, ch) in text.chars().enumerate() {
            let glyph = font.glyph(ch).ok_or(RenderError::Unsupported)?;
            let glyph_x = i64::from(position.x)
                + i64::try_from(index).map_err(|_| RenderError::ArithmeticOverflow)?
                    * i64::from(advance);
            if glyph_x < i64::from(i32::MIN) || glyph_x > i64::from(i32::MAX) {
                return Err(RenderError::InvalidRect);
            }
            for row in 0..font.height {
                let bits =
                    glyph.rows[usize::try_from(row).map_err(|_| RenderError::InvalidRect)?];
                for col in 0..font.width {
                    let mask_shift = font
                        .width
                        .checked_sub(1)
                        .and_then(|value| value.checked_sub(col))
                        .ok_or(RenderError::ArithmeticOverflow)?;
                    if (bits & (1u8 << mask_shift)) != 0 {
                        let x = glyph_x + i64::from(col);
                        let y = i64::from(position.y) + i64::from(row);
                        if x >= i64::from(i32::MIN)
                            && x <= i64::from(i32::MAX)
                            && y >= i64::from(i32::MIN)
                            && y <= i64::from(i32::MAX)
                        {
                            self.set_pixel_checked(x as i32, y as i32, color)?;
                        }
                    }
                }
            }
        }
        damage.add(bounds)
    }
}
