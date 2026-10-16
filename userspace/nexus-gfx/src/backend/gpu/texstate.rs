// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A texture's descriptor block (TASK-0215 5c-2 P5d-1): the image and sampler words the shader
//! core's texture unit reads, written by the device for any stride texture — a client's surface,
//! a plane of the ring, a rectangle of one — from the open driver's texture-state description
//! (`IMAGE_WORD0`, `STRIDE_IMAGE_WORD1`, `SAMPLER_WORD0`/`1`). The block is the one the
//! pipelines' state load copies (`rogue::setup::TEXTURE_DESCRIPTOR_DWORDS`): the image words at
//! its first dword (sh4 in the bundle's layout), the sampler words at its ninth (sh12). The open
//! driver's block also carries the image's byte size and its query data (dwords 4–7) and a second
//! sampler copy (dwords 16–19) for shaders that ask a texture's size; ours never ask, and leave
//! them zero. The render bundle's captured descriptor is the golden (`tests/gpu_texstate.rs`).

/// The descriptor block's dwords, as the state load copies it.
pub const DESCRIPTOR_DWORDS: usize = 20;
const SAMPLER_AT: usize = 8;

/// Texel formats.
pub mod format {
    /// Four 8-bit unsigned normalised channels in one word.
    pub const U8U8U8U8: u8 = 12;
}

/// Where each channel of a texel comes from.
pub mod swizzle {
    pub const IDENTITY: [u8; 4] = [0, 1, 2, 3];
    /// A word whose bytes are blue, green, red, alpha (the planes) read as red, green, blue,
    /// alpha.
    pub const BGRA: [u8; 4] = [2, 1, 0, 3];
    pub const ONE: u8 = 4;
    pub const ZERO: u8 = 5;
}

const TEXTYPE_STRIDE: u64 = 4;
const FILTER_POINT: u64 = 0;
const FILTER_LINEAR: u64 = 1;
const ADDR_REPEAT: u64 = 0;
const ADDR_CLAMP_TO_EDGE: u64 = 2;
/// The LOD bias 0.0 in the field's 5.8 format.
const DADJUST_ZERO: u64 = 4095;

/// A stride texture of one level: `width` by `height` texels of `format` at `address`, its rows
/// `stride` texels apart, its channels through `swizzle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    pub address: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u8,
    pub swizzle: [u8; 4],
}

/// How a texture is sampled: bilinear or point, clamped to its edge or repeated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sampler {
    pub linear: bool,
    pub clamp: bool,
}

impl Sampler {
    /// Bilinear, clamped to the edge (the compositor's sampler).
    pub const BILINEAR_CLAMP: Self = Self { linear: true, clamp: true };
}

/// Why a texture has no descriptor: a value its field cannot hold, named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TexError(pub &'static str);

/// `v` into `word` at `start..=end`, refused when wider.
fn put(word: &mut u64, start: u32, end: u32, v: u64, what: &'static str) -> Result<(), TexError> {
    let bits = end - start + 1;
    if v >> bits != 0 {
        return Err(TexError(what));
    }
    *word |= v << start;
    Ok(())
}

/// The image words: `IMAGE_WORD0` and `STRIDE_IMAGE_WORD1`.
fn image_words(img: &Image) -> Result<[u64; 2], TexError> {
    if img.width == 0 || img.height == 0 || img.stride < img.width {
        return Err(TexError("size"));
    }
    if img.address % 4 != 0 {
        return Err(TexError("address"));
    }
    // A channel, one or zero: the field's other values name nothing.
    if img.swizzle.iter().any(|c| *c > swizzle::ZERO) {
        return Err(TexError("swizzle"));
    }
    let mut w0 = 0;
    put(&mut w0, 0, 2, TEXTYPE_STRIDE, "type")?;
    put(&mut w0, 5, 7, u64::from(img.swizzle[3]), "swizzle")?;
    put(&mut w0, 8, 10, u64::from(img.swizzle[2]), "swizzle")?;
    put(&mut w0, 11, 13, u64::from(img.swizzle[1]), "swizzle")?;
    put(&mut w0, 14, 16, u64::from(img.swizzle[0]), "swizzle")?;
    put(&mut w0, 27, 33, u64::from(img.format), "format")?;
    put(&mut w0, 34, 47, u64::from(img.width - 1), "width")?;
    put(&mut w0, 48, 61, u64::from(img.height - 1), "height")?;
    let mut w1 = 0;
    put(&mut w1, 0, 14, u64::from(img.stride - 1), "stride")?;
    put(&mut w1, 16, 53, img.address >> 2, "address")?;
    // Alpha in the word's top byte, as in every format here; one level.
    put(&mut w1, 59, 59, 1, "alpha position")?;
    put(&mut w1, 60, 63, 1, "levels")?;
    Ok([w0, w1])
}

/// `SAMPLER_WORD0`; `SAMPLER_WORD1` stays zero (it names further planes).
fn sampler_word(s: &Sampler) -> Result<u64, TexError> {
    let filter = if s.linear { FILTER_LINEAR } else { FILTER_POINT };
    let addr = if s.clamp { ADDR_CLAMP_TO_EDGE } else { ADDR_REPEAT };
    let mut w = 0;
    put(&mut w, 0, 12, DADJUST_ZERO, "lod bias")?;
    put(&mut w, 36, 37, filter, "filter")?;
    put(&mut w, 38, 39, filter, "filter")?;
    put(&mut w, 41, 43, addr, "address mode")?;
    put(&mut w, 44, 46, addr, "address mode")?;
    put(&mut w, 56, 58, addr, "address mode")?;
    Ok(w)
}

/// The descriptor block of `img` sampled by `s`.
pub fn descriptor(img: &Image, s: &Sampler) -> Result<[u32; DESCRIPTOR_DWORDS], TexError> {
    let [w0, w1] = image_words(img)?;
    let s0 = sampler_word(s)?;
    let mut d = [0u32; DESCRIPTOR_DWORDS];
    for (i, w) in [w0, w1].iter().enumerate() {
        d[2 * i] = *w as u32;
        d[2 * i + 1] = (*w >> 32) as u32;
    }
    d[SAMPLER_AT] = s0 as u32;
    d[SAMPLER_AT + 1] = (s0 >> 32) as u32;
    Ok(d)
}

/// The block as bytes, for the device's write.
pub fn descriptor_bytes(img: &Image, s: &Sampler) -> Result<[u8; 4 * DESCRIPTOR_DWORDS], TexError> {
    let d = descriptor(img, s)?;
    let mut b = [0u8; 4 * DESCRIPTOR_DWORDS];
    for (i, w) in d.iter().enumerate() {
        b[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    Ok(b)
}
