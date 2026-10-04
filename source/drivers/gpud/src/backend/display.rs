// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What gpud's request loop drives (RFC-0093 §5): ONE display behind the one protocol —
//! the virtio GPU on QEMU (its 2D scanout or its virgl GL scanout) or the board's display
//! controller (TASK-0251 P2a step 2). The loop owns the wire: it decodes every request, validates
//! a present's commands, derives its damage, prints the chain trace and the present statistics,
//! latches the reveal and encodes every answer. A display owns its device: how its framebuffer
//! is made, how a present reaches the glass, what the cursor is, when the splash lets go.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every QEMU display lane (the virtio GPU); the board ladder (the controller)

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::command::buffer::{Command, CommittedBuffer};

use crate::service_stats::PresentStats;

pub(crate) trait Display {
    /// The visible mode (RFC-0098 C7): what the grant and the attach ack carry.
    fn mode(&self) -> (u32, u32);

    /// The framebuffer to grant (its VMO): made once FOR the scanout device, then the same
    /// object for gpud's life. `None` names the refusal on the console.
    fn framebuffer(&mut self) -> Option<u32>;

    /// `OP_SET_FRAMEBUFFER_VMO`: windowd's first frame is in the granted framebuffer. A refusal
    /// is the status the attach ack carries.
    fn attach(&mut self) -> Result<(), u8>;

    /// After an acked attach: what the device does once windowd owns the frames.
    fn attached(&mut self) {}

    /// A present's validated commands, into the framebuffer (the chain's G3 hop).
    fn execute(&mut self, commands: &[Command]) -> Result<(), GfxError>;

    /// `damage` of the display plane to the glass (the chain's G4 hop).
    fn scan_out(&mut self, damage: Rect) -> Result<(), GfxError>;

    /// The first frame reached the glass (the chain trace ends): a display with a readback
    /// proves it there.
    fn first_frame_shown(&mut self) {}

    /// Opens a present for [`Display::present_lost`].
    fn begin_present(&mut self) {}

    /// Whether the device lost commands of the present opened by [`Display::begin_present`]
    /// although every call returned success: the present is then NACKed and windowd re-sends
    /// the damage instead of booking a frame nobody saw.
    fn present_lost(&mut self) -> bool {
        false
    }

    /// `OP_UPLOAD_CURSOR`: the status and the reply magic naming the cursor path windowd takes.
    fn upload_cursor(&mut self, bgra: &[u8], w: u32, h: u32, hot: (u32, u32)) -> (u8, Option<u32>);

    /// `OP_UPLOAD_CURSOR_SHAPE`: fill a shape slot (arms nothing).
    fn cache_cursor_shape(
        &mut self,
        id: u8,
        bgra: &[u8],
        w: u32,
        h: u32,
        hot: (u32, u32),
    ) -> Result<(), GfxError>;

    /// `OP_SELECT_CURSOR_SHAPE`: a cached shape becomes the cursor.
    fn select_cursor_shape(&mut self, id: u8) -> Result<(), GfxError>;

    /// `OP_MOVE_CURSOR`: where the pointer is.
    fn move_cursor(&mut self, x: i32, y: i32) -> Result<(), GfxError>;

    /// `OP_UPLOAD_ICON`: a sprite the display composites itself, at `dst` on screen.
    fn upload_icon(&mut self, bgra: &[u8], w: u32, h: u32, dst: Rect) -> Result<(), GfxError>;

    /// `OP_SUBMIT_ANIMATION_FRAME`: commands into the framebuffer outside a present.
    fn submit(&mut self, commands: CommittedBuffer) -> Result<(), GfxError>;

    /// `OP_SET_LAYER_SCROLL`: record a layer's source row. `Ok(true)` = the display re-composites
    /// its retained layers on [`Display::flush_layer_overrides`] (the loop drains the burst
    /// first); `Ok(false)` = it retains no layer set — the next full present carries the row.
    fn set_layer_scroll(&mut self, _scroll_id: u32, _src_row: u32) -> Result<bool, GfxError> {
        Ok(false)
    }

    /// `OP_SET_LAYER_TRANSFORM`: as [`Display::set_layer_scroll`], for a layer's transform — its
    /// translation, the opacity it multiplies by and its scale about its centre (percent).
    fn set_layer_transform(
        &mut self,
        _layer_id: u32,
        _offset: (i16, i16),
        _opacity: u8,
        _scale_pct: u16,
    ) -> Result<bool, GfxError> {
        Ok(false)
    }

    /// Re-composite the recorded overrides once (the burst is drained).
    fn flush_layer_overrides(&mut self) {}

    /// `OP_WALLPAPER_DIRTY`: windowd rewrote the wallpaper plane.
    fn wallpaper_dirty(&mut self) {}

    /// `OP_REVEAL` (RFC-0093 §5): the desktop is complete; the next present may reveal it.
    fn request_reveal(&mut self);

    /// Whether windowd asked for the reveal.
    fn reveal_requested(&self) -> bool;

    /// Whether the boot splash still holds the glass (a present then is composed, not shown).
    fn holding_splash(&self) -> bool;

    /// Whether the display needs self-paced frames now (the frame clock, TASK-0324 P7-d).
    fn pacing(&self) -> bool {
        false
    }

    /// One self-paced frame; `attached` = windowd's framebuffer is attached.
    fn frame_tick(&mut self, _attached: bool, _stats: &mut PresentStats) {}
}
