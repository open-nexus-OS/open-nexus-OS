// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Repo-owned fixture font parser for deterministic text rendering.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use crate::error::{RenderError, RenderResult};

const FIXTURE_FONT: &str = include_str!("../../fonts/fixture_font_5x7.txt");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureFont {
    pub(crate) width: u32,
    pub(crate) height: u32,
    glyphs: Vec<Glyph>,
}

impl FixtureFont {
    pub fn load_default() -> RenderResult<Self> {
        Self::parse(FIXTURE_FONT)
    }

    pub fn parse(input: &str) -> RenderResult<Self> {
        let mut width = None;
        let mut height = None;
        let mut glyphs = Vec::new();
        let mut active: Option<(char, Vec<u8>)> = None;

        for raw in input.lines() {
            let line = raw.trim_end();
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("FONT ") {
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("WIDTH ") {
                width = Some(parse_u32(value)?);
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("HEIGHT ") {
                height = Some(parse_u32(value)?);
                continue;
            }
            if let Some(value) = line.strip_prefix("GLYPH ") {
                if active.is_some() {
                    return Err(RenderError::FixtureFontRejected);
                }
                let glyph_char = parse_glyph_char(value)?;
                active = Some((glyph_char, Vec::new()));
                continue;
            }
            if trimmed == "END" {
                let (ch, rows) = active.take().ok_or(RenderError::FixtureFontRejected)?;
                let height = height.ok_or(RenderError::FixtureFontRejected)?;
                if rows.len()
                    != usize::try_from(height).map_err(|_| RenderError::FixtureFontRejected)?
                    || glyphs.iter().any(|glyph: &Glyph| glyph.ch == ch)
                {
                    return Err(RenderError::FixtureFontRejected);
                }
                glyphs.push(Glyph { ch, rows });
                continue;
            }

            let Some((_, rows)) = active.as_mut() else {
                return Err(RenderError::FixtureFontRejected);
            };
            let width = width.ok_or(RenderError::FixtureFontRejected)?;
            rows.push(parse_glyph_row(trimmed, width)?);
        }

        if active.is_some() {
            return Err(RenderError::FixtureFontRejected);
        }
        let width = width.ok_or(RenderError::FixtureFontRejected)?;
        let height = height.ok_or(RenderError::FixtureFontRejected)?;
        if width == 0 || width > 8 || height == 0 || glyphs.is_empty() {
            return Err(RenderError::FixtureFontRejected);
        }
        Ok(Self {
            width,
            height,
            glyphs,
        })
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn glyph(&self, ch: char) -> Option<&Glyph> {
        self.glyphs.iter().find(|glyph| glyph.ch == ch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub(crate) ch: char,
    pub(crate) rows: Vec<u8>,
}

fn parse_u32(value: &str) -> RenderResult<u32> {
    let mut out = 0u32;
    for ch in value.chars() {
        let digit = ch.to_digit(10).ok_or(RenderError::FixtureFontRejected)?;
        out = out
            .checked_mul(10)
            .and_then(|value| value.checked_add(digit))
            .ok_or(RenderError::FixtureFontRejected)?;
    }
    Ok(out)
}

fn parse_glyph_char(value: &str) -> RenderResult<char> {
    if value == " " || value == "SPACE" {
        return Ok(' ');
    }
    let mut chars = value.chars();
    let ch = chars.next().ok_or(RenderError::FixtureFontRejected)?;
    if chars.next().is_some() {
        return Err(RenderError::FixtureFontRejected);
    }
    Ok(ch)
}

fn parse_glyph_row(row: &str, width: u32) -> RenderResult<u8> {
    if u32::try_from(row.len()).map_err(|_| RenderError::FixtureFontRejected)? != width {
        return Err(RenderError::FixtureFontRejected);
    }
    let mut bits = 0u8;
    for ch in row.chars() {
        bits = bits
            .checked_shl(1)
            .ok_or(RenderError::FixtureFontRejected)?;
        match ch {
            '0' => {}
            '1' => bits |= 1,
            _ => return Err(RenderError::FixtureFontRejected),
        }
    }
    Ok(bits)
}
