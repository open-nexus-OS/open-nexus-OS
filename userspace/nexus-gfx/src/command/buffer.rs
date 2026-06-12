// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::command::render_encoder::RenderCommandEncoder;
use crate::core::error::GfxError;
use crate::core::types::{RenderPassDesc, TileRect};
use alloc::vec::Vec;

pub const MAX_COMMANDS: usize = 1024;
pub const MAX_FRAGMENT_BYTES: usize = 4096;
pub const MAX_TILE_RECTS: usize = 1024;
pub const MAX_RENDER_EXTENT: u32 = 8192;

/// Packed RGBA color for rendering commands.
/// Stored as u32 LE: [R, G, B, A] in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbaColor(pub u32);

impl RgbaColor {
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(u32::from_le_bytes([r, g, b, a]))
    }

    #[must_use]
    pub const fn r(self) -> u8 {
        self.0.to_le_bytes()[0]
    }

    #[must_use]
    pub const fn g(self) -> u8 {
        self.0.to_le_bytes()[1]
    }

    #[must_use]
    pub const fn b(self) -> u8 {
        self.0.to_le_bytes()[2]
    }

    #[must_use]
    pub const fn a(self) -> u8 {
        self.0.to_le_bytes()[3]
    }

    #[must_use]
    pub const fn from_u32(v: u32) -> Self {
        Self(v)
    }

    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn as_array(self) -> [u8; 4] {
        [self.r(), self.g(), self.b(), self.a()]
    }
}

/// Internal command representation — not exposed to users.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Set shader uniform data at a byte offset.
    SetFragmentBytes { offset: usize, data: Vec<u8> },
    /// Fill rectangular tiles with a solid color (e.g. bitmap-font glyph pixels).
    DrawTiles { tiles: Vec<TileRect>, color: RgbaColor },
    /// Copy a rectangle from the source surface to the framebuffer.
    /// All coordinates are in the render-pass coordinate space.
    BlitSurface { src_x: u32, src_y: u32, dst_x: u32, dst_y: u32, width: u32, height: u32 },
    /// Fill an SDF rounded rectangle with a solid color.
    FillSdfRoundedRect { rect: TileRect, radius: u32, color: RgbaColor },
    /// Apply a box blur followed by saturation boost to a framebuffer region.
    BlurBackdrop { rect: TileRect, radius: u32, saturation_percent: u32 },
    /// Blend the cursor bitmap onto the framebuffer at the given position.
    BlendCursor { x: u32, y: u32, width: u32, height: u32 },
    /// Blit between arbitrary absolute VMO rows — bypasses the display_y_offset
    /// adjustment that BlitSurface applies. Used to write/read the Plane 3 blur
    /// cache: writing cached blur (display→Plane3) and reading it back (Plane3→display).
    BlitAbsolute { src_x: u32, src_y_abs: u32, dst_x: u32, dst_y_abs: u32, width: u32, height: u32 },
}

/// A recorded command buffer. Use begin_render_pass() to start a render pass.
#[derive(Debug, Clone)]
pub struct CommandBuffer {
    pub(crate) commands: Vec<Command>,
    render_extent: Option<(u32, u32)>,
}

impl CommandBuffer {
    pub fn new() -> Self {
        Self { commands: Vec::new(), render_extent: None }
    }

    /// Begin a render pass. Returns a RenderCommandEncoder for recording.
    pub fn begin_render_pass(&mut self, desc: RenderPassDesc) -> RenderCommandEncoder<'_> {
        self.try_begin_render_pass(desc).expect("invalid render pass")
    }

    pub fn try_begin_render_pass(
        &mut self,
        desc: RenderPassDesc,
    ) -> Result<RenderCommandEncoder<'_>, GfxError> {
        validate_render_pass(&desc)?;
        self.render_extent = Some((desc.width, desc.height));
        Ok(RenderCommandEncoder::new(self))
    }

    #[must_use]
    pub fn commit(self) -> CommittedBuffer {
        self.try_commit().expect("invalid command buffer")
    }

    pub fn try_commit(self) -> Result<CommittedBuffer, GfxError> {
        validate_commands(&self.commands, self.render_extent)?;
        Ok(CommittedBuffer { commands: self.commands })
    }

    pub(crate) fn push_command(&mut self, command: Command) -> Result<(), GfxError> {
        if self.commands.len() >= MAX_COMMANDS {
            return Err(GfxError::ResourceExhausted);
        }
        validate_command(&command, self.render_extent)?;
        self.commands.push(command);
        Ok(())
    }

    /// Backing command-vector capacity. Test/diagnostic hook used to assert that
    /// frame-to-frame reuse (clear + re-record) performs no reallocation.
    #[doc(hidden)]
    #[must_use]
    pub fn command_capacity(&self) -> usize {
        self.commands.capacity()
    }

    /// Clear all recorded commands while retaining the backing allocation.
    ///
    /// This is the key to reusing one `CommandBuffer` across frames under a
    /// non-freeing (bump) allocator: `Vec::clear` keeps capacity, so a buffer
    /// that has reached steady-state size records subsequent frames without any
    /// new heap allocation. Resets the render extent so a fresh render pass must
    /// be opened before recording again.
    pub fn clear(&mut self) {
        self.commands.clear();
        self.render_extent = None;
    }

    /// Serialize the recorded commands into `buf` without consuming the buffer.
    ///
    /// Mirrors [`CommittedBuffer::serialize_into`] but borrows `self`, so a
    /// reusable `CommandBuffer` can be encoded each frame and then [`clear`]ed
    /// for the next one — no `commit()`/drop cycle, hence no per-frame alloc.
    ///
    /// [`clear`]: CommandBuffer::clear
    pub fn serialize_into(&self, buf: &mut [u8]) -> Result<usize, GfxError> {
        validate_commands(&self.commands, self.render_extent)?;
        serialize_commands(&self.commands, buf)
    }
}

impl Default for CommandBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// A sealed command buffer — immutable, ready for backend submission.
/// Transferred by value; ownership moves to the backend on submit.
#[derive(Debug, Clone, PartialEq)]
pub struct CommittedBuffer {
    pub(crate) commands: Vec<Command>,
}

// ── Wire format tags ──────────────────────────────────────────────
const TAG_SET_FRAGMENT: u8 = 0;
const TAG_DRAW_TILES: u8 = 1;
const TAG_BLIT_SURFACE: u8 = 2;
const TAG_FILL_SDF_ROUNDED_RECT: u8 = 3;
const TAG_BLUR_BACKDROP: u8 = 4;
const TAG_BLEND_CURSOR: u8 = 5;
const TAG_BLIT_ABSOLUTE: u8 = 6;

/// Maximum serialized size for a CommittedBuffer (guard against overflow).
// Sized to fit gpud's 8192-byte present-frame receive buffer (minus the 1-byte
// opcode prefix and headroom). Frames carry the full scene-graph CB including
// bitmap-text DrawTiles runs, which exceed the old 2048 cap.
const MAX_SERIALIZED: usize = 8000;

impl CommittedBuffer {
    /// Access the commands for backend inspection/execution.
    #[must_use]
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    #[must_use]
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    pub fn validate(&self) -> Result<(), GfxError> {
        validate_commands(&self.commands, None)
    }

    /// Backing command-vector capacity. Test/diagnostic hook used to assert that
    /// `reload_from` reuse performs no per-frame reallocation.
    #[doc(hidden)]
    #[must_use]
    pub fn command_capacity(&self) -> usize {
        self.commands.capacity()
    }

    /// Serialize into a pre-allocated buffer. Returns the number of bytes written.
    pub fn serialize_into(&self, buf: &mut [u8]) -> Result<usize, GfxError> {
        serialize_commands(&self.commands, buf)
    }

    /// Construct an empty buffer with pre-allocated command capacity.
    ///
    /// Pair with [`reload_from`] to parse a frame each iteration without
    /// reallocating — essential for consumers on a non-freeing bump allocator
    /// (e.g. gpud deserializing one present frame per vsync).
    ///
    /// [`reload_from`]: CommittedBuffer::reload_from
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { commands: Vec::with_capacity(capacity) }
    }

    /// Deserialize from a byte slice. Returns the CommittedBuffer and bytes consumed.
    pub fn deserialize_from(buf: &[u8]) -> Result<(Self, usize), GfxError> {
        let mut commands = Vec::new();
        let consumed = decode_commands_into(buf, &mut commands)?;
        let cb = CommittedBuffer { commands };
        cb.validate()?;
        Ok((cb, consumed))
    }

    /// Re-parse `buf` into this buffer in place, reusing the existing command
    /// allocation (`Vec::clear` retains capacity). Returns bytes consumed.
    ///
    /// Allocation-free counterpart to [`deserialize_from`] once warmed up: the
    /// command `Vec` is cleared and refilled rather than freshly allocated, so a
    /// consumer that holds one `CommittedBuffer` and calls `reload_from` per
    /// frame performs zero heap allocation in steady state. This is what keeps
    /// gpud from exhausting its bump heap while presenting animation frames.
    ///
    /// [`deserialize_from`]: CommittedBuffer::deserialize_from
    pub fn reload_from(&mut self, buf: &[u8]) -> Result<usize, GfxError> {
        let consumed = decode_commands_into(buf, &mut self.commands)?;
        self.validate()?;
        Ok(consumed)
    }
}

/// Parse a serialized command stream into `out`, reusing its capacity.
/// `out` is cleared first. Shared by [`CommittedBuffer::deserialize_from`] and
/// [`CommittedBuffer::reload_from`]. Does not validate — callers validate the
/// assembled buffer (so validation sees the whole command set at once).
fn decode_commands_into(buf: &[u8], out: &mut Vec<Command>) -> Result<usize, GfxError> {
    out.clear();
    if buf.len() < 2 {
        return Err(GfxError::InvalidArgument);
    }
    let cmd_count = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    let mut pos = 2usize;
    for _ in 0..cmd_count {
        if buf.len() <= pos {
            return Err(GfxError::InvalidArgument);
        }
        let tag = buf[pos];
        pos += 1;
        let (cmd, n) = match tag {
            TAG_SET_FRAGMENT => deser_fragment(buf, pos)?,
            TAG_DRAW_TILES => deser_tiles(buf, pos)?,
            TAG_BLIT_SURFACE => deser_blit(buf, pos)?,
            TAG_FILL_SDF_ROUNDED_RECT => deser_sdf_rect(buf, pos, false)?,
            TAG_BLUR_BACKDROP => deser_sdf_rect(buf, pos, true)?,
            TAG_BLEND_CURSOR => deser_cursor(buf, pos)?,
            TAG_BLIT_ABSOLUTE => deser_blit_absolute(buf, pos)?,
            _ => return Err(GfxError::InvalidArgument),
        };
        pos = n;
        out.push(cmd);
    }
    Ok(pos)
}

// ── Serialization helpers ────────────────────────────────────────

/// Serialize a command slice into `buf`. Shared by [`CommittedBuffer::serialize_into`]
/// and [`CommandBuffer::serialize_into`] so both wire-encode identically.
fn serialize_commands(commands: &[Command], buf: &mut [u8]) -> Result<usize, GfxError> {
    if commands.len() > u16::MAX as usize {
        return Err(GfxError::ResourceExhausted);
    }
    let cmd_count = commands.len() as u16;
    let mut pos = 0usize;
    if buf.len() < 2 {
        return Err(GfxError::ResourceExhausted);
    }
    buf[0..2].copy_from_slice(&cmd_count.to_le_bytes());
    pos += 2;
    for cmd in commands {
        match cmd {
            Command::SetFragmentBytes { offset, data } => {
                pos = ser_tag_data(buf, pos, TAG_SET_FRAGMENT, *offset, data)?;
            }
            Command::DrawTiles { tiles, color } => {
                pos = ser_tiles(buf, pos, TAG_DRAW_TILES, tiles, *color)?;
            }
            Command::BlitSurface { src_x, src_y, dst_x, dst_y, width, height } => {
                pos = ser_blit(
                    buf,
                    pos,
                    TAG_BLIT_SURFACE,
                    *src_x,
                    *src_y,
                    *dst_x,
                    *dst_y,
                    *width,
                    *height,
                )?;
            }
            Command::FillSdfRoundedRect { rect, radius, color } => {
                pos = ser_sdf_rect(buf, pos, TAG_FILL_SDF_ROUNDED_RECT, rect, *radius, *color)?;
            }
            Command::BlurBackdrop { rect, radius, saturation_percent } => {
                pos = ser_sdf_rect(
                    buf,
                    pos,
                    TAG_BLUR_BACKDROP,
                    rect,
                    *radius,
                    RgbaColor::from_u32(*saturation_percent),
                )?;
            }
            Command::BlendCursor { x, y, width, height } => {
                pos = ser_cursor(buf, pos, TAG_BLEND_CURSOR, *x, *y, *width, *height)?;
            }
            Command::BlitAbsolute { src_x, src_y_abs, dst_x, dst_y_abs, width, height } => {
                pos = ser_blit(
                    buf,
                    pos,
                    TAG_BLIT_ABSOLUTE,
                    *src_x,
                    *src_y_abs,
                    *dst_x,
                    *dst_y_abs,
                    *width,
                    *height,
                )?;
            }
        }
    }
    Ok(pos)
}

fn ser_tag_data(
    buf: &mut [u8],
    pos: usize,
    tag: u8,
    offset: usize,
    data: &[u8],
) -> Result<usize, GfxError> {
    let off = u16::try_from(offset).map_err(|_| GfxError::InvalidArgument)?;
    let len = u16::try_from(data.len()).map_err(|_| GfxError::InvalidArgument)?;
    let needed = pos + 5 + data.len();
    if buf.len() < needed || needed > MAX_SERIALIZED {
        return Err(GfxError::ResourceExhausted);
    }
    buf[pos] = tag;
    buf[pos + 1..pos + 3].copy_from_slice(&off.to_le_bytes());
    buf[pos + 3..pos + 5].copy_from_slice(&len.to_le_bytes());
    buf[pos + 5..pos + 5 + data.len()].copy_from_slice(data);
    Ok(pos + 5 + data.len())
}

fn ser_tiles(
    buf: &mut [u8],
    pos: usize,
    tag: u8,
    tiles: &[TileRect],
    color: RgbaColor,
) -> Result<usize, GfxError> {
    if tiles.len() > u16::MAX as usize {
        return Err(GfxError::ResourceExhausted);
    }
    let tile_bytes = tiles.len() * 16;
    // tag(1) + count(2) + color(4) + tiles.
    let needed = pos + 7 + tile_bytes;
    if buf.len() < needed || needed > MAX_SERIALIZED {
        return Err(GfxError::ResourceExhausted);
    }
    buf[pos] = tag;
    let count = tiles.len() as u16;
    buf[pos + 1..pos + 3].copy_from_slice(&count.to_le_bytes());
    buf[pos + 3..pos + 7].copy_from_slice(&color.as_u32().to_le_bytes());
    let mut p = pos + 7;
    for t in tiles {
        buf[p..p + 4].copy_from_slice(&t.x.to_le_bytes());
        buf[p + 4..p + 8].copy_from_slice(&t.y.to_le_bytes());
        buf[p + 8..p + 12].copy_from_slice(&t.width.to_le_bytes());
        buf[p + 12..p + 16].copy_from_slice(&t.height.to_le_bytes());
        p += 16;
    }
    Ok(p)
}

fn ser_blit(
    buf: &mut [u8],
    pos: usize,
    tag: u8,
    sx: u32,
    sy: u32,
    dx: u32,
    dy: u32,
    w: u32,
    h: u32,
) -> Result<usize, GfxError> {
    let needed = pos + 25;
    if buf.len() < needed || needed > MAX_SERIALIZED {
        return Err(GfxError::ResourceExhausted);
    }
    buf[pos] = tag;
    buf[pos + 1..pos + 5].copy_from_slice(&sx.to_le_bytes());
    buf[pos + 5..pos + 9].copy_from_slice(&sy.to_le_bytes());
    buf[pos + 9..pos + 13].copy_from_slice(&dx.to_le_bytes());
    buf[pos + 13..pos + 17].copy_from_slice(&dy.to_le_bytes());
    buf[pos + 17..pos + 21].copy_from_slice(&w.to_le_bytes());
    buf[pos + 21..pos + 25].copy_from_slice(&h.to_le_bytes());
    Ok(pos + 25)
}

fn ser_sdf_rect(
    buf: &mut [u8],
    pos: usize,
    tag: u8,
    rect: &TileRect,
    radius: u32,
    color_or_sat: RgbaColor,
) -> Result<usize, GfxError> {
    let needed = pos + 25;
    if buf.len() < needed || needed > MAX_SERIALIZED {
        return Err(GfxError::ResourceExhausted);
    }
    buf[pos] = tag;
    buf[pos + 1..pos + 5].copy_from_slice(&rect.x.to_le_bytes());
    buf[pos + 5..pos + 9].copy_from_slice(&rect.y.to_le_bytes());
    buf[pos + 9..pos + 13].copy_from_slice(&rect.width.to_le_bytes());
    buf[pos + 13..pos + 17].copy_from_slice(&rect.height.to_le_bytes());
    buf[pos + 17..pos + 21].copy_from_slice(&radius.to_le_bytes());
    buf[pos + 21..pos + 25].copy_from_slice(&color_or_sat.0.to_le_bytes());
    Ok(pos + 25)
}

fn ser_cursor(
    buf: &mut [u8],
    pos: usize,
    tag: u8,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> Result<usize, GfxError> {
    let needed = pos + 17;
    if buf.len() < needed || needed > MAX_SERIALIZED {
        return Err(GfxError::ResourceExhausted);
    }
    buf[pos] = tag;
    buf[pos + 1..pos + 5].copy_from_slice(&x.to_le_bytes());
    buf[pos + 5..pos + 9].copy_from_slice(&y.to_le_bytes());
    buf[pos + 9..pos + 13].copy_from_slice(&w.to_le_bytes());
    buf[pos + 13..pos + 17].copy_from_slice(&h.to_le_bytes());
    Ok(pos + 17)
}

// ── Deserialization helpers ──────────────────────────────────────

fn deser_fragment(buf: &[u8], pos: usize) -> Result<(Command, usize), GfxError> {
    if buf.len() < pos + 4 {
        return Err(GfxError::InvalidArgument);
    }
    let offset = u16::from_le_bytes([buf[pos], buf[pos + 1]]) as usize;
    let data_len = u16::from_le_bytes([buf[pos + 2], buf[pos + 3]]) as usize;
    let dpos = pos + 4;
    if buf.len() < dpos + data_len || data_len > MAX_FRAGMENT_BYTES {
        return Err(GfxError::InvalidArgument);
    }
    let data = buf[dpos..dpos + data_len].to_vec();
    Ok((Command::SetFragmentBytes { offset, data }, dpos + data_len))
}

fn deser_tiles(buf: &[u8], pos: usize) -> Result<(Command, usize), GfxError> {
    if buf.len() < pos + 6 {
        return Err(GfxError::InvalidArgument);
    }
    let count = u16::from_le_bytes([buf[pos], buf[pos + 1]]) as usize;
    let color = RgbaColor::from_u32(u32::from_le_bytes([
        buf[pos + 2],
        buf[pos + 3],
        buf[pos + 4],
        buf[pos + 5],
    ]));
    let tpos = pos + 6;
    let bytes = count * 16;
    if buf.len() < tpos + bytes {
        return Err(GfxError::InvalidArgument);
    }
    let mut tiles = Vec::new();
    for i in 0..count {
        let b = tpos + i * 16;
        tiles.push(TileRect {
            x: u32::from_le_bytes([buf[b], buf[b + 1], buf[b + 2], buf[b + 3]]),
            y: u32::from_le_bytes([buf[b + 4], buf[b + 5], buf[b + 6], buf[b + 7]]),
            width: u32::from_le_bytes([buf[b + 8], buf[b + 9], buf[b + 10], buf[b + 11]]),
            height: u32::from_le_bytes([buf[b + 12], buf[b + 13], buf[b + 14], buf[b + 15]]),
        });
    }
    Ok((Command::DrawTiles { tiles, color }, tpos + bytes))
}

fn deser_blit(buf: &[u8], pos: usize) -> Result<(Command, usize), GfxError> {
    if buf.len() < pos + 24 {
        return Err(GfxError::InvalidArgument);
    }
    Ok((
        Command::BlitSurface {
            src_x: u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]),
            src_y: u32::from_le_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]),
            dst_x: u32::from_le_bytes([buf[pos + 8], buf[pos + 9], buf[pos + 10], buf[pos + 11]]),
            dst_y: u32::from_le_bytes([buf[pos + 12], buf[pos + 13], buf[pos + 14], buf[pos + 15]]),
            width: u32::from_le_bytes([buf[pos + 16], buf[pos + 17], buf[pos + 18], buf[pos + 19]]),
            height: u32::from_le_bytes([
                buf[pos + 20],
                buf[pos + 21],
                buf[pos + 22],
                buf[pos + 23],
            ]),
        },
        pos + 24,
    ))
}

fn deser_blit_absolute(buf: &[u8], pos: usize) -> Result<(Command, usize), GfxError> {
    if buf.len() < pos + 24 {
        return Err(GfxError::InvalidArgument);
    }
    Ok((
        Command::BlitAbsolute {
            src_x: u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]),
            src_y_abs: u32::from_le_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]),
            dst_x: u32::from_le_bytes([buf[pos + 8], buf[pos + 9], buf[pos + 10], buf[pos + 11]]),
            dst_y_abs: u32::from_le_bytes([
                buf[pos + 12],
                buf[pos + 13],
                buf[pos + 14],
                buf[pos + 15],
            ]),
            width: u32::from_le_bytes([buf[pos + 16], buf[pos + 17], buf[pos + 18], buf[pos + 19]]),
            height: u32::from_le_bytes([
                buf[pos + 20],
                buf[pos + 21],
                buf[pos + 22],
                buf[pos + 23],
            ]),
        },
        pos + 24,
    ))
}

fn deser_sdf_rect(buf: &[u8], pos: usize, is_blur: bool) -> Result<(Command, usize), GfxError> {
    if buf.len() < pos + 24 {
        return Err(GfxError::InvalidArgument);
    }
    let rect = TileRect {
        x: u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]),
        y: u32::from_le_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]),
        width: u32::from_le_bytes([buf[pos + 8], buf[pos + 9], buf[pos + 10], buf[pos + 11]]),
        height: u32::from_le_bytes([buf[pos + 12], buf[pos + 13], buf[pos + 14], buf[pos + 15]]),
    };
    let radius = u32::from_le_bytes([buf[pos + 16], buf[pos + 17], buf[pos + 18], buf[pos + 19]]);
    let v = u32::from_le_bytes([buf[pos + 20], buf[pos + 21], buf[pos + 22], buf[pos + 23]]);
    let cmd = if is_blur {
        Command::BlurBackdrop { rect, radius, saturation_percent: v }
    } else {
        Command::FillSdfRoundedRect { rect, radius, color: RgbaColor::from_u32(v) }
    };
    Ok((cmd, pos + 24))
}

fn deser_cursor(buf: &[u8], pos: usize) -> Result<(Command, usize), GfxError> {
    if buf.len() < pos + 16 {
        return Err(GfxError::InvalidArgument);
    }
    Ok((
        Command::BlendCursor {
            x: u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]),
            y: u32::from_le_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]),
            width: u32::from_le_bytes([buf[pos + 8], buf[pos + 9], buf[pos + 10], buf[pos + 11]]),
            height: u32::from_le_bytes([
                buf[pos + 12],
                buf[pos + 13],
                buf[pos + 14],
                buf[pos + 15],
            ]),
        },
        pos + 16,
    ))
}

// ── Validation ───────────────────────────────────────────────────

fn validate_render_pass(desc: &RenderPassDesc) -> Result<(), GfxError> {
    if desc.width == 0
        || desc.height == 0
        || desc.width > MAX_RENDER_EXTENT
        || desc.height > MAX_RENDER_EXTENT
    {
        return Err(GfxError::InvalidArgument);
    }
    Ok(())
}

fn validate_commands(
    commands: &[Command],
    render_extent: Option<(u32, u32)>,
) -> Result<(), GfxError> {
    if commands.len() > MAX_COMMANDS {
        return Err(GfxError::ResourceExhausted);
    }
    for command in commands {
        validate_command(command, render_extent)?;
    }
    Ok(())
}

fn validate_command(command: &Command, render_extent: Option<(u32, u32)>) -> Result<(), GfxError> {
    match command {
        Command::SetFragmentBytes { offset, data } => {
            let end = offset.checked_add(data.len()).ok_or(GfxError::InvalidArgument)?;
            if data.len() > MAX_FRAGMENT_BYTES || end > MAX_FRAGMENT_BYTES {
                return Err(GfxError::ResourceExhausted);
            }
            Ok(())
        }
        Command::DrawTiles { tiles, .. } => validate_tile_list(tiles, render_extent),
        Command::BlitSurface { width, height, .. } => {
            if *width == 0 || *height == 0 {
                return Err(GfxError::InvalidArgument);
            }
            Ok(())
        }
        Command::FillSdfRoundedRect { rect, .. } | Command::BlurBackdrop { rect, .. } => {
            validate_tile(*rect, render_extent)
        }
        Command::BlendCursor { width, height, .. } => {
            if *width == 0 || *height == 0 {
                return Err(GfxError::InvalidArgument);
            }
            Ok(())
        }
        Command::BlitAbsolute { width, height, .. } => {
            if *width == 0 || *height == 0 {
                return Err(GfxError::InvalidArgument);
            }
            Ok(())
        }
    }
}

fn validate_tile_list(
    tiles: &[TileRect],
    render_extent: Option<(u32, u32)>,
) -> Result<(), GfxError> {
    if tiles.is_empty() || tiles.len() > MAX_TILE_RECTS {
        return Err(GfxError::InvalidArgument);
    }
    for tile in tiles {
        validate_tile(*tile, render_extent)?;
    }
    Ok(())
}

fn validate_tile(tile: TileRect, render_extent: Option<(u32, u32)>) -> Result<(), GfxError> {
    if tile.width == 0 || tile.height == 0 {
        return Err(GfxError::InvalidArgument);
    }
    let end_x = tile.x.checked_add(tile.width).ok_or(GfxError::InvalidArgument)?;
    let end_y = tile.y.checked_add(tile.height).ok_or(GfxError::InvalidArgument)?;
    if let Some((width, height)) = render_extent {
        if end_x > width || end_y > height {
            return Err(GfxError::InvalidArgument);
        }
    }
    Ok(())
}
