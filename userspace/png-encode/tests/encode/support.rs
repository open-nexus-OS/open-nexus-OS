// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: shared fixtures of the png-encode integration tests: deterministic BGRA frames
//! (flat, gradients, noise, a UI-like desktop) with junk in alpha and row padding, an
//! independent decode through the `png` crate with CRC-32 and Adler-32 verification on, a
//! bitwise CRC-32 of its own, a chunk parser, and sinks that record or refuse writes.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: support module (no tests of its own)

use png_encode::{BgraFrame, Encoder, Sink};

/// Deterministic noise (xorshift32).
pub struct XorShift(pub u32);

impl XorShift {
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    pub fn byte(&mut self) -> u8 {
        (self.next_u32() >> 24) as u8
    }
}

/// A position hash for fixtures that must not depend on iteration order.
fn mix(a: u32, b: u32) -> u32 {
    let mut rng = XorShift((a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B)) | 1);
    rng.next_u32();
    rng.next_u32()
}

/// A BGRA8 frame owned by a test.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub pixels: Vec<u8>,
}

impl Image {
    /// Tight rows; pixel colours from `f(x, y) = [r, g, b]`.
    pub fn from_fn(width: u32, height: u32, f: impl FnMut(u32, u32) -> [u8; 3]) -> Self {
        Self::with_stride(width, height, width as usize * 4, f)
    }

    /// Rows `stride` bytes apart, the buffer as short as allowed (no padding after the last
    /// row). Alpha and padding hold junk the encoder must ignore.
    pub fn with_stride(
        width: u32,
        height: u32,
        stride: usize,
        mut f: impl FnMut(u32, u32) -> [u8; 3],
    ) -> Self {
        let len = stride * (height as usize - 1) + width as usize * 4;
        let mut junk = XorShift(0xDEAD_BEEF);
        let mut pixels: Vec<u8> = (0..len).map(|_| junk.byte()).collect();
        for y in 0..height {
            for x in 0..width {
                let [r, g, b] = f(x, y);
                let at = y as usize * stride + x as usize * 4;
                pixels[at..at + 3].copy_from_slice(&[b, g, r]);
            }
        }
        Self { width, height, stride, pixels }
    }

    pub fn frame(&self) -> BgraFrame<'_> {
        BgraFrame::new(&self.pixels, self.width, self.height, self.stride)
    }

    /// What a decoder must return: RGB8, tight rows.
    pub fn rgb(&self) -> Vec<u8> {
        let row = self.width as usize * 4;
        self.pixels
            .chunks(self.stride)
            .flat_map(|line| line[..row].chunks_exact(4).flat_map(|px| [px[2], px[1], px[0]]))
            .collect()
    }

    /// The caller's raw frame size: width x height x 4 bytes.
    pub fn bgra_len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

pub fn flat(width: u32, height: u32, rgb: [u8; 3]) -> Image {
    Image::from_fn(width, height, |_, _| rgb)
}

pub fn horizontal_gradient(width: u32, height: u32) -> Image {
    Image::from_fn(width, height, |x, _| {
        let v = (x * 255 / (width - 1).max(1)) as u8;
        [v, 255 - v, v / 2 + 64]
    })
}

pub fn vertical_gradient(width: u32, height: u32) -> Image {
    Image::from_fn(width, height, |_, y| {
        let v = (y * 255 / (height - 1).max(1)) as u8;
        [255 - v, v / 3 + 40, v]
    })
}

pub fn noise(width: u32, height: u32, seed: u32) -> Image {
    let mut rng = XorShift(seed);
    Image::from_fn(width, height, |_, _| [rng.byte(), rng.byte(), rng.byte()])
}

/// A desktop: a soft wallpaper, two overlapping windows (border, title bar with a title,
/// body with lines of glyph-like specks), a taskbar with icons.
pub fn ui_like(width: u32, height: u32) -> Image {
    // (x, y, w, h, title colour, body colour); the later window is on top.
    let windows = [
        (
            width / 16,
            height / 12,
            width / 2,
            height * 3 / 5,
            [0x2B, 0x3A, 0x55],
            [0xF4, 0xF5, 0xF7],
        ),
        (width * 2 / 5, height / 4, width / 2, height / 2, [0x33, 0x55, 0x33], [0xFF, 0xFF, 0xFF]),
    ];
    let ink = [0x22, 0x24, 0x28];
    Image::from_fn(width, height, |x, y| {
        if y + 40 >= height {
            let icon = x % 48 >= 8 && x % 48 < 40 && y + 32 >= height && y + 8 < height;
            let shade = (mix(x / 48, 7) % 200) as u8;
            return if icon { [shade, 255 - shade, 0x80] } else { [0x20, 0x22, 0x28] };
        }
        for &(wx, wy, ww, wh, title, body) in windows.iter().rev() {
            if x < wx || y < wy || x >= wx + ww || y >= wy + wh {
                continue;
            }
            let (lx, ly) = (x - wx, y - wy);
            if lx == 0 || ly == 0 || lx + 1 == ww || ly + 1 == wh {
                return [0x10, 0x10, 0x10];
            }
            let text = |top: u32, ink: [u8; 3], bg: [u8; 3]| {
                let (tx, ty) = (lx.saturating_sub(12), ly - top);
                let in_line = ty % 20 >= 4 && ty % 20 < 16 && lx >= 12 && lx + 12 < ww;
                let word_gap = mix(tx / 8, ty / 20) % 7 == 0;
                let speck = tx % 8 < 6 && mix(x, y) % 3 == 0;
                if in_line && !word_gap && speck {
                    ink
                } else {
                    bg
                }
            };
            return if ly < 28 {
                if lx < ww / 3 {
                    text(0, [0xFF, 0xFF, 0xFF], title)
                } else {
                    title
                }
            } else {
                text(28, ink, body)
            };
        }
        [0x30 + (y / 40) as u8, 0x60, 0x90 - (y / 50) as u8]
    })
}

/// Encodes with a fresh encoder sized exactly for the image.
pub fn encode(image: &Image) -> Vec<u8> {
    let mut encoder = Encoder::new(image.width).expect("encoder");
    encode_with(&mut encoder, image)
}

pub fn encode_with(encoder: &mut Encoder, image: &Image) -> Vec<u8> {
    let mut png = Vec::new();
    let written = encoder.encode(image.frame(), &mut png).expect("encode");
    assert_eq!(written, png.len() as u64, "returned byte count");
    png
}

/// Decodes through the `png` crate with every checksum verified: (width, height, RGB8).
pub fn decode(png_bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut options = png::DecodeOptions::default();
    options.set_ignore_checksums(false);
    let mut reader =
        png::Decoder::new_with_options(png_bytes, options).read_info().expect("header decodes");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("image data decodes");
    assert_eq!(info.color_type, png::ColorType::Rgb);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    buf.truncate(info.buffer_size());
    reader.finish().expect("trailing chunks decode");
    (info.width, info.height, buf)
}

/// Encode, decode independently, compare every pixel; returns the PNG.
pub fn assert_round_trip(image: &Image) -> Vec<u8> {
    let png = encode(image);
    let (width, height, rgb) = decode(&png);
    assert_eq!((width, height), (image.width, image.height));
    assert!(rgb == image.rgb(), "pixels differ for {}x{}", image.width, image.height);
    png
}

/// CRC-32 bit by bit, independent of the crate's table.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// One parsed chunk.
pub struct Chunk<'a> {
    pub kind: [u8; 4],
    pub payload: &'a [u8],
    pub crc_ok: bool,
}

/// The chunks of `bytes` (which must hold whole chunks only).
pub fn parse_chunks(mut bytes: &[u8]) -> Vec<Chunk<'_>> {
    let mut chunks = Vec::new();
    while !bytes.is_empty() {
        let len = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
        let crc = u32::from_be_bytes(bytes[8 + len..12 + len].try_into().unwrap());
        chunks.push(Chunk {
            kind: bytes[4..8].try_into().unwrap(),
            payload: &bytes[8..8 + len],
            crc_ok: crc == crc32(&bytes[4..8 + len]),
        });
        bytes = &bytes[12 + len..];
    }
    chunks
}

/// Records every write separately.
#[derive(Default)]
pub struct RecordingSink {
    pub writes: Vec<Vec<u8>>,
}

impl Sink for RecordingSink {
    type Error = core::convert::Infallible;

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.writes.push(bytes.to_vec());
        Ok(())
    }
}

/// Refuses its `fail_at`-th write (0-based) and every write after it.
pub struct FailingSink {
    pub fail_at: usize,
    pub accepted: usize,
    pub attempts: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Refused {
    pub at: usize,
}

impl Sink for FailingSink {
    type Error = Refused;

    fn write(&mut self, _bytes: &[u8]) -> Result<(), Refused> {
        self.attempts += 1;
        if self.accepted == self.fail_at {
            return Err(Refused { at: self.accepted });
        }
        self.accepted += 1;
        Ok(())
    }
}
