// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `windowd` state machine for surfaces, scene commits, present scheduling, and input routing.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Behavior and reject coverage in `ui_windowd_host` and `ui_v2a_host`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::vec;
use alloc::vec::Vec;

use crate::buffer::{validate_buffer, PixelFormat, SurfaceBuffer};
use crate::error::{Result, WindowdError};
use crate::frame::{blit_surface, Frame, Layer};
use crate::geometry::{
    checked_len, checked_stride, validate_damage, validate_dimensions, Rect, MAX_DAMAGE_RECTS,
    MAX_LAYERS, MAX_SURFACES,
};
use crate::ids::{CallerCtx, CommitSeq, FenceId, FrameIndex, InputSeq, PresentSeq, SurfaceId};

pub(crate) const DEFAULT_WIDTH: u32 = 64;
pub(crate) const DEFAULT_HEIGHT: u32 = 48;
pub(crate) const DEFAULT_HZ: u16 = 60;
pub const VISIBLE_BOOTSTRAP_WIDTH: u32 = 1280;
pub const VISIBLE_BOOTSTRAP_HEIGHT: u32 = 800;
pub const VISIBLE_BOOTSTRAP_HZ: u16 = 120;
pub const VISIBLE_BOOTSTRAP_FORMAT: PixelFormat = PixelFormat::Bgra8888;
pub const VISIBLE_CURSOR_BGRA: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
pub const VISIBLE_HOVER_BGRA: [u8; 4] = [0x30, 0xa0, 0xff, 0xff];
pub const VISIBLE_FOCUS_BGRA: [u8; 4] = [0x00, 0xff, 0xff, 0xff];
const MAX_BACK_BUFFERS_PER_SURFACE: usize = 2;
const MAX_FENCES: usize = 64;
const MAX_INPUT_EVENTS: usize = 32;
/// RFC-0055: deterministic pointer-motion coalescing – max burst within one coalescing window.
const MAX_POINTER_COALESCE_BURST: u64 = 8;
/// RFC-0055: how many consecutive no-damage/no-visible-change skips are allowed before forced present.
const MAX_NO_DAMAGE_SKIPS: u64 = 4;
/// RFC-0055: max wakeup collapses without visible update before forcing a present.
const MAX_IDLE_CHEAP_WAKEUPS: u64 = 6;
/// RFC-0055: fence coalescing limit per pending present.
const MAX_FENCES_PER_PRESENT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "present ack carries damage and sequence; ignoring hides frame progress"]
pub struct PresentAck {
    pub seq: PresentSeq,
    pub damage_rects: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackBufferLease {
    pub surface: SurfaceId,
    pub frame_index: FrameIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentFrameAck {
    pub fence_id: FenceId,
    pub frame_index: FrameIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "scheduled present ack carries coalesced frames and latency; ignoring hides perf regressions"]
pub struct ScheduledPresentAck {
    pub seq: PresentSeq,
    pub damage_rects: u16,
    pub frames_coalesced: u16,
    pub fences_signaled: u16,
    pub latency_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentFenceStatus {
    pub fence_id: FenceId,
    pub frame_index: FrameIndex,
    pub signaled: bool,
    pub coalesced: bool,
    pub present_seq: Option<PresentSeq>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEventKind {
    PointerMove { x: i32, y: i32 },
    PointerDown,
    Keyboard { key_code: u32 },
    TouchDown { x: i32, y: i32 },
    TouchMove { x: i32, y: i32 },
    TouchUp { x: i32, y: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchInputPhase {
    Down,
    Move,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "input delivery carries event routing result; dropping hides delivery failures"]
pub struct InputDelivery {
    pub seq: InputSeq,
    pub surface: SurfaceId,
    pub kind: InputEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiProfile {
    Desktop,
    Mobile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputStubStatus {
    UnsupportedStub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowdConfig {
    pub width: u32,
    pub height: u32,
    pub hz: u16,
}

impl Default for WindowdConfig {
    fn default() -> Self {
        Self { width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT, hz: DEFAULT_HZ }
    }
}

impl WindowdConfig {
    pub const fn visible_bootstrap() -> Self {
        Self {
            width: VISIBLE_BOOTSTRAP_WIDTH,
            height: VISIBLE_BOOTSTRAP_HEIGHT,
            hz: VISIBLE_BOOTSTRAP_HZ,
        }
    }
}

#[derive(Debug, Clone)]
struct Surface {
    id: SurfaceId,
    owner: crate::CallerId,
    buffer: SurfaceBuffer,
    damage: Vec<Rect>,
    back_buffers: Vec<BackBuffer>,
    pending_present: Option<PendingPresent>,
    last_presented_frame: Option<FrameIndex>,
}

#[derive(Debug, Clone)]
struct BackBuffer {
    frame_index: FrameIndex,
    buffer: SurfaceBuffer,
}

#[derive(Debug, Clone)]
struct PendingPresent {
    frame_index: FrameIndex,
    buffer: SurfaceBuffer,
    damage: Vec<Rect>,
    fence_id: FenceId,
    submitted_tick: u64,
    coalesced_frames: u16,
    coalesced_fences: Vec<FenceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FenceState {
    Pending,
    Signaled { present_seq: PresentSeq, coalesced: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FenceRecord {
    id: FenceId,
    frame_index: FrameIndex,
    state: FenceState,
}

#[derive(Debug, Clone)]
pub struct WindowServer {
    config: WindowdConfig,
    surfaces: Vec<Surface>,
    layers: Vec<Layer>,
    next_surface_id: u64,
    next_commit_seq: u64,
    next_present_seq: u64,
    initialized: bool,
    systemui_loaded: bool,
    last_frame: Option<Frame>,
    last_present: Option<PresentAck>,
    next_fence_id: u64,
    next_input_seq: u64,
    scheduler_tick: u64,
    last_scheduled_present: Option<ScheduledPresentAck>,
    fences: Vec<FenceRecord>,
    input_events: Vec<InputDelivery>,
    focused_surface: Option<SurfaceId>,
    input_enabled: bool,
    scheduler_enabled: bool,
    last_pointer_hit: Option<SurfaceId>,
    pointer_position: Option<PointerPosition>,
    /// RFC-0055: last coalesced pointer position for frame compositing.
    last_coalesced_pointer: Option<PointerPosition>,
    /// RFC-0055: number of coalesced pointer-motion events in current burst window.
    pointer_coalesce_burst: u64,
    /// RFC-0055: consecutive no-damage present ticks skipped.
    no_damage_skips: u64,
    /// RFC-0055: consecutive idle-cheap wakeups without visible update.
    idle_cheap_wakeups: u64,
    /// RFC-0055: last composed frame pixel hash for skip eligibility.
    last_frame_hash: Option<u64>,
    /// RFC-0055: pointer-motion coalescing enabled.
    fastpath_enabled: bool,
}

impl WindowServer {
    pub fn new(config: WindowdConfig) -> Result<Self> {
        validate_dimensions(config.width, config.height)?;
        if config.hz == 0 {
            return Err(WindowdError::InvalidDimensions);
        }
        Ok(Self {
            config,
            surfaces: Vec::new(),
            layers: Vec::new(),
            next_surface_id: 1,
            next_commit_seq: 1,
            next_present_seq: 1,
            initialized: true,
            systemui_loaded: false,
            last_frame: None,
            last_present: None,
            next_fence_id: 1,
            next_input_seq: 1,
            scheduler_tick: 0,
            last_scheduled_present: None,
            fences: Vec::new(),
            input_events: Vec::new(),
            focused_surface: None,
            input_enabled: false,
            scheduler_enabled: false,
            last_pointer_hit: None,
            pointer_position: None,
            last_coalesced_pointer: None,
            pointer_coalesce_burst: 0,
            no_damage_skips: 0,
            idle_cheap_wakeups: 0,
            last_frame_hash: None,
            fastpath_enabled: false,
        })
    }

    pub const fn config(&self) -> WindowdConfig {
        self.config
    }

    pub const fn initialized(&self) -> bool {
        self.initialized
    }

    pub const fn systemui_loaded(&self) -> bool {
        self.systemui_loaded
    }

    pub fn load_systemui(&mut self, profile: UiProfile) -> Result<()> {
        match profile {
            UiProfile::Desktop | UiProfile::Mobile => {
                self.systemui_loaded = true;
                Ok(())
            }
        }
    }

    pub fn create_surface(
        &mut self,
        caller: CallerCtx,
        buffer: SurfaceBuffer,
    ) -> Result<SurfaceId> {
        if self.surfaces.len() >= MAX_SURFACES {
            return Err(WindowdError::TooManySurfaces);
        }
        validate_buffer(caller, &buffer)?;
        let id = SurfaceId::new(self.next_surface_id);
        self.next_surface_id =
            self.next_surface_id.checked_add(1).ok_or(WindowdError::ArithmeticOverflow)?;
        self.surfaces.push(Surface {
            id,
            owner: caller.caller_id(),
            buffer,
            damage: Vec::new(),
            back_buffers: Vec::new(),
            pending_present: None,
            last_presented_frame: None,
        });
        Ok(id)
    }

    /// Frees a client surface and its buffer (free-on-close).
    ///
    /// RFC-0065: each app owns its surface VMO; when the app is closed/stopped its
    /// surface is destroyed and its memory reclaimed, rather than persisting in a
    /// shared plane. Owner-gated; idempotent-friendly (`StaleSurfaceId` if gone).
    pub fn destroy_surface(&mut self, caller: CallerCtx, surface_id: SurfaceId) -> Result<()> {
        let surface = self.surface(surface_id).ok_or(WindowdError::StaleSurfaceId)?;
        if surface.owner != caller.caller_id() {
            return Err(WindowdError::Unauthorized);
        }
        self.surfaces.retain(|s| s.id != surface_id);
        Ok(())
    }

    pub fn queue_buffer(
        &mut self,
        caller: CallerCtx,
        surface_id: SurfaceId,
        buffer: SurfaceBuffer,
        damage: &[Rect],
    ) -> Result<()> {
        validate_buffer(caller, &buffer)?;
        validate_damage(buffer.width, buffer.height, damage)?;
        let surface = self.surface_mut(surface_id).ok_or(WindowdError::StaleSurfaceId)?;
        if surface.owner != caller.caller_id() {
            return Err(WindowdError::Unauthorized);
        }
        if surface.buffer.width != buffer.width || surface.buffer.height != buffer.height {
            return Err(WindowdError::InvalidDimensions);
        }
        surface.buffer = buffer;
        surface.damage.clear();
        surface.damage.extend_from_slice(damage);
        Ok(())
    }

    pub fn resize_surface(
        &mut self,
        caller: CallerCtx,
        surface_id: SurfaceId,
        buffer: SurfaceBuffer,
        damage: &[Rect],
    ) -> Result<()> {
        validate_buffer(caller, &buffer)?;
        validate_damage(buffer.width, buffer.height, damage)?;
        let surface = self.surface_mut(surface_id).ok_or(WindowdError::StaleSurfaceId)?;
        if surface.owner != caller.caller_id() {
            return Err(WindowdError::Unauthorized);
        }
        surface.buffer = buffer;
        surface.damage.clear();
        surface.damage.extend_from_slice(damage);
        Ok(())
    }

    pub fn acquire_back_buffer(
        &mut self,
        caller: CallerCtx,
        surface_id: SurfaceId,
        frame_index: FrameIndex,
        buffer: SurfaceBuffer,
    ) -> Result<BackBufferLease> {
        if frame_index.raw() == 0 {
            return Err(WindowdError::InvalidFrameIndex);
        }
        validate_buffer(caller, &buffer)?;
        let surface = self.surface_mut(surface_id).ok_or(WindowdError::StaleSurfaceId)?;
        if surface.owner != caller.caller_id() {
            return Err(WindowdError::Unauthorized);
        }
        if surface.buffer.width != buffer.width || surface.buffer.height != buffer.height {
            return Err(WindowdError::InvalidDimensions);
        }
        if surface.last_presented_frame.is_some_and(|last| frame_index <= last)
            || surface
                .pending_present
                .as_ref()
                .is_some_and(|pending| frame_index <= pending.frame_index)
        {
            return Err(WindowdError::StalePresentSequence);
        }
        if surface.back_buffers.iter().any(|back| back.frame_index == frame_index) {
            return Err(WindowdError::InvalidFrameIndex);
        }
        if surface.back_buffers.len() >= MAX_BACK_BUFFERS_PER_SURFACE {
            return Err(WindowdError::SchedulerQueueFull);
        }
        surface.back_buffers.push(BackBuffer { frame_index, buffer });
        self.scheduler_enabled = true;
        Ok(BackBufferLease { surface: surface_id, frame_index })
    }

    pub fn present_frame(
        &mut self,
        caller: CallerCtx,
        surface_id: SurfaceId,
        frame_index: FrameIndex,
        damage: &[Rect],
    ) -> Result<PresentFrameAck> {
        if frame_index.raw() == 0 {
            return Err(WindowdError::InvalidFrameIndex);
        }
        let surface_idx = self.surface_index(surface_id).ok_or(WindowdError::StaleSurfaceId)?;
        if self.surfaces[surface_idx].owner != caller.caller_id() {
            return Err(WindowdError::Unauthorized);
        }
        if self.surfaces[surface_idx].last_presented_frame.is_some_and(|last| frame_index <= last)
            || self.surfaces[surface_idx]
                .pending_present
                .as_ref()
                .is_some_and(|pending| frame_index <= pending.frame_index)
        {
            return Err(WindowdError::StalePresentSequence);
        }
        let back_idx = self.surfaces[surface_idx]
            .back_buffers
            .iter()
            .position(|back| back.frame_index == frame_index)
            .ok_or(WindowdError::InvalidFrameIndex)?;
        let buffer = &self.surfaces[surface_idx].back_buffers[back_idx].buffer;
        validate_damage(buffer.width, buffer.height, damage)?;
        if let Some(pending) = &self.surfaces[surface_idx].pending_present {
            let combined = pending
                .damage
                .len()
                .checked_add(damage.len())
                .ok_or(WindowdError::ArithmeticOverflow)?;
            if combined > MAX_DAMAGE_RECTS {
                return Err(WindowdError::TooManyDamageRects);
            }
        }
        if self.fences.len() >= MAX_FENCES {
            return Err(WindowdError::SchedulerQueueFull);
        }
        let fence_id = FenceId::new(self.next_fence_id);
        self.next_fence_id =
            self.next_fence_id.checked_add(1).ok_or(WindowdError::ArithmeticOverflow)?;
        let surface = &mut self.surfaces[surface_idx];
        let back = surface.back_buffers.remove(back_idx);
        let mut coalesced_fences = Vec::new();
        let mut coalesced_frames = 0u16;
        let mut merged_damage = Vec::new();
        if let Some(previous) = surface.pending_present.take() {
            if previous.coalesced_fences.len().saturating_add(1) >= MAX_FENCES_PER_PRESENT {
                return Err(WindowdError::SchedulerQueueFull);
            }
            coalesced_fences.extend_from_slice(&previous.coalesced_fences);
            coalesced_fences.push(previous.fence_id);
            coalesced_frames =
                previous.coalesced_frames.checked_add(1).ok_or(WindowdError::ArithmeticOverflow)?;
            merged_damage.extend_from_slice(&previous.damage);
        }
        merged_damage.extend_from_slice(damage);
        surface.pending_present = Some(PendingPresent {
            frame_index,
            buffer: back.buffer,
            damage: merged_damage,
            fence_id,
            submitted_tick: self.scheduler_tick,
            coalesced_frames,
            coalesced_fences,
        });
        self.fences.push(FenceRecord { id: fence_id, frame_index, state: FenceState::Pending });
        self.scheduler_enabled = true;
        Ok(PresentFrameAck { fence_id, frame_index })
    }

    pub fn commit_scene(
        &mut self,
        caller: CallerCtx,
        seq: CommitSeq,
        layers: &[Layer],
    ) -> Result<()> {
        if caller.caller_id() != crate::CallerId::system() {
            return Err(WindowdError::Unauthorized);
        }
        if seq.raw() != self.next_commit_seq {
            return Err(WindowdError::StaleCommitSequence);
        }
        if layers.is_empty() || layers.len() > MAX_LAYERS {
            return Err(WindowdError::TooManyLayers);
        }
        for layer in layers {
            if self.surface(layer.surface).is_none() {
                return Err(WindowdError::StaleSurfaceId);
            }
        }
        let mut next_layers = layers.to_vec();
        next_layers.sort_by_key(|layer| (layer.z, layer.surface.raw()));
        self.layers = next_layers;
        self.next_commit_seq =
            self.next_commit_seq.checked_add(1).ok_or(WindowdError::ArithmeticOverflow)?;
        Ok(())
    }

    #[must_use = "present outcome must be checked: None means no damage, Some means frame composed"]
    pub fn present_tick(&mut self) -> Result<Option<PresentAck>> {
        if self.layers.is_empty() {
            return Err(WindowdError::NoCommittedScene);
        }
        let damage_count = self.total_damage_count()?;
        if damage_count == 0 {
            return Ok(None);
        }
        let frame = self.compose_frame()?;
        let ack =
            PresentAck { seq: PresentSeq::new(self.next_present_seq), damage_rects: damage_count };
        self.next_present_seq =
            self.next_present_seq.checked_add(1).ok_or(WindowdError::ArithmeticOverflow)?;
        for surface in &mut self.surfaces {
            surface.damage.clear();
        }
        self.last_frame = Some(frame);
        self.last_present = Some(ack);
        Ok(Some(ack))
    }

    pub fn present_bootstrap_scanout_tick(&mut self) -> Result<PresentAck> {
        if self.layers.is_empty() {
            return Err(WindowdError::NoCommittedScene);
        }
        let damage_count = self.total_damage_count()?;
        if damage_count == 0 {
            return Err(WindowdError::MarkerBeforePresentState);
        }
        #[cfg(not(all(nexus_env = "os", target_os = "none")))]
        let frame = self.compose_frame()?;
        let ack =
            PresentAck { seq: PresentSeq::new(self.next_present_seq), damage_rects: damage_count };
        self.next_present_seq =
            self.next_present_seq.checked_add(1).ok_or(WindowdError::ArithmeticOverflow)?;
        for surface in &mut self.surfaces {
            surface.damage.clear();
        }
        #[cfg(not(all(nexus_env = "os", target_os = "none")))]
        {
            self.last_frame = Some(frame);
        }
        self.last_present = Some(ack);
        Ok(ack)
    }

    pub fn present_scheduler_tick(&mut self) -> Result<Option<ScheduledPresentAck>> {
        if self.layers.is_empty() {
            return Err(WindowdError::NoCommittedScene);
        }
        self.scheduler_tick =
            self.scheduler_tick.checked_add(1).ok_or(WindowdError::ArithmeticOverflow)?;
        let mut damage_count: usize = 0;
        let mut coalesced_frames: u16 = 0;
        let mut fences_to_signal: Vec<(FenceId, bool)> = Vec::new();
        let mut earliest_submitted_tick = self.scheduler_tick;
        for surface in &mut self.surfaces {
            if let Some(pending) = surface.pending_present.take() {
                damage_count = damage_count
                    .checked_add(pending.damage.len())
                    .ok_or(WindowdError::ArithmeticOverflow)?;
                coalesced_frames = coalesced_frames
                    .checked_add(pending.coalesced_frames)
                    .ok_or(WindowdError::ArithmeticOverflow)?;
                earliest_submitted_tick = earliest_submitted_tick.min(pending.submitted_tick);
                for fence_id in pending.coalesced_fences {
                    fences_to_signal.push((fence_id, true));
                }
                fences_to_signal.push((pending.fence_id, false));
                surface.buffer = pending.buffer;
                surface.damage.clear();
                surface.damage.extend_from_slice(&pending.damage);
                surface.last_presented_frame = Some(pending.frame_index);
            } else {
                // Direct surface damage (from queue_buffer, not present_frame)
                damage_count = damage_count
                    .checked_add(surface.damage.len())
                    .ok_or(WindowdError::ArithmeticOverflow)?;
            }
        }
        if damage_count == 0 {
            return Ok(None);
        }
        // Clear surface damage after counting (for direct damage path)
        for surface in &mut self.surfaces {
            surface.damage.clear();
        }
        let damage_rects =
            u16::try_from(damage_count).map_err(|_| WindowdError::TooManyDamageRects)?;
        let frame = self.compose_frame()?;
        let present_seq = PresentSeq::new(self.next_present_seq);
        self.next_present_seq =
            self.next_present_seq.checked_add(1).ok_or(WindowdError::ArithmeticOverflow)?;
        for (fence_id, coalesced) in &fences_to_signal {
            self.signal_fence(*fence_id, present_seq, *coalesced);
        }
        for surface in &mut self.surfaces {
            surface.damage.clear();
        }
        let elapsed_ticks = self.scheduler_tick.saturating_sub(earliest_submitted_tick);
        let latency_ms = elapsed_ticks
            .saturating_mul(1000)
            .checked_div(u64::from(self.config.hz))
            .unwrap_or(0)
            .min(u64::from(u32::MAX)) as u32;
        let ack = ScheduledPresentAck {
            seq: present_seq,
            damage_rects,
            frames_coalesced: coalesced_frames,
            fences_signaled: u16::try_from(fences_to_signal.len())
                .map_err(|_| WindowdError::TooManyDamageRects)?,
            latency_ms,
        };
        self.last_frame = Some(frame);
        self.last_present = Some(PresentAck { seq: present_seq, damage_rects });
        self.last_scheduled_present = Some(ack);
        self.scheduler_enabled = true;
        Ok(Some(ack))
    }

    pub fn present_fence_status(&self, fence_id: FenceId) -> Result<PresentFenceStatus> {
        let record = self
            .fences
            .iter()
            .find(|record| record.id == fence_id)
            .ok_or(WindowdError::StalePresentSequence)?;
        let (signaled, coalesced, present_seq) = match record.state {
            FenceState::Pending => (false, false, None),
            FenceState::Signaled { present_seq, coalesced } => (true, coalesced, Some(present_seq)),
        };
        Ok(PresentFenceStatus {
            fence_id: record.id,
            frame_index: record.frame_index,
            signaled,
            coalesced,
            present_seq,
        })
    }

    pub fn subscribe_vsync(&self, last_seen: PresentSeq) -> Result<Option<PresentAck>> {
        let ack = self.marker_evidence()?;
        if ack.seq > last_seen {
            Ok(Some(ack))
        } else {
            Ok(None)
        }
    }

    pub fn subscribe_input_stub(
        &self,
        caller: CallerCtx,
        surface_id: SurfaceId,
    ) -> Result<InputStubStatus> {
        let surface = self.surface(surface_id).ok_or(WindowdError::StaleSurfaceId)?;
        if surface.owner != caller.caller_id() {
            return Err(WindowdError::Unauthorized);
        }
        Ok(InputStubStatus::UnsupportedStub)
    }

    #[must_use = "routing result carries input delivery; dropping hides event loss"]
    pub fn route_pointer_move(&mut self, x: i32, y: i32) -> Result<InputDelivery> {
        let position = self.validate_pointer_position(x, y)?;
        let surface_id = self.hit_test(x, y).ok_or(WindowdError::StaleSurfaceId)?;
        self.ensure_input_capacity()?;
        self.reset_coalesce_burst();
        self.pointer_position = Some(position);
        self.last_pointer_hit = Some(surface_id);
        self.input_enabled = true;
        self.push_input_delivery(surface_id, InputEventKind::PointerMove { x, y })
    }

    pub fn route_pointer_down(&mut self, x: i32, y: i32) -> Result<InputDelivery> {
        let position = self.validate_pointer_position(x, y)?;
        let surface_id = self.hit_test(x, y).ok_or(WindowdError::StaleSurfaceId)?;
        self.ensure_input_capacity()?;
        self.reset_coalesce_burst();
        self.pointer_position = Some(position);
        self.focused_surface = Some(surface_id);
        self.last_pointer_hit = Some(surface_id);
        self.input_enabled = true;
        self.push_input_delivery(surface_id, InputEventKind::PointerDown)
    }

    pub fn route_keyboard(&mut self, key_code: u32) -> Result<InputDelivery> {
        let surface_id = self.focused_surface.ok_or(WindowdError::NoFocusedSurface)?;
        if self.surface(surface_id).is_none() {
            return Err(WindowdError::StaleSurfaceId);
        }
        self.ensure_input_capacity()?;
        self.reset_coalesce_burst();
        self.input_enabled = true;
        self.push_input_delivery(surface_id, InputEventKind::Keyboard { key_code })
    }

    pub fn route_touch(&mut self, x: i32, y: i32, phase: TouchInputPhase) -> Result<InputDelivery> {
        let position = self.validate_pointer_position(x, y)?;
        let surface_id = self.hit_test(x, y).ok_or(WindowdError::StaleSurfaceId)?;
        self.ensure_input_capacity()?;
        self.reset_coalesce_burst();
        self.pointer_position = Some(position);
        self.last_pointer_hit = Some(surface_id);
        self.input_enabled = true;
        if matches!(phase, TouchInputPhase::Down) {
            self.focused_surface = Some(surface_id);
        }
        let kind = match phase {
            TouchInputPhase::Down => InputEventKind::TouchDown { x, y },
            TouchInputPhase::Move => InputEventKind::TouchMove { x, y },
            TouchInputPhase::Up => InputEventKind::TouchUp { x, y },
        };
        self.push_input_delivery(surface_id, kind)
    }

    pub fn take_input_events(
        &mut self,
        caller: CallerCtx,
        surface_id: SurfaceId,
    ) -> Result<Vec<InputDelivery>> {
        let surface = self.surface(surface_id).ok_or(WindowdError::StaleSurfaceId)?;
        if surface.owner != caller.caller_id() {
            return Err(WindowdError::Unauthorized);
        }
        let mut delivered = Vec::new();
        let mut retained = Vec::new();
        for event in self.input_events.drain(..) {
            if event.surface == surface_id {
                delivered.push(event);
            } else {
                retained.push(event);
            }
        }
        self.input_events = retained;
        Ok(delivered)
    }

    /// RFC-0055: Enable pointer-motion coalescing fastpath.
    pub fn enable_fastpath(&mut self) {
        self.fastpath_enabled = true;
    }

    /// RFC-0055: Whether fastpath coalescing is active.
    pub const fn fastpath_enabled(&self) -> bool {
        self.fastpath_enabled
    }

    /// RFC-0055: Attempt to coalesce a pointer-move event.
    ///
    /// Returns `Ok(true)` if the event is subsumed within the current burst budget.
    /// The pointer position is stored for frame compositing even when coalesced.
    /// Returns `Err(CoalesceBurstExceeded)` if the burst limit is hit
    /// (caller must emit a real `route_pointer_move` to reset the window).
    /// Returns `Err(FastPathDisabled)` if `enable_fastpath()` was not called.
    #[must_use = "coalesce result must be checked: true means skipped, false means full route needed"]
    pub fn try_coalesce_pointer_move(&mut self, x: i32, y: i32) -> Result<bool> {
        if !self.fastpath_enabled {
            return Err(WindowdError::FastPathDisabled);
        }
        self.pointer_coalesce_burst = self.pointer_coalesce_burst.saturating_add(1);
        if self.pointer_coalesce_burst > MAX_POINTER_COALESCE_BURST {
            return Err(WindowdError::CoalesceBurstExceeded);
        }
        self.last_coalesced_pointer = Some(PointerPosition { x, y });
        self.pointer_position = Some(PointerPosition { x, y });
        Ok(true)
    }

    /// RFC-0055: Reset the pointer coalesce burst counter and last coalesced position.
    /// Called after each present and at semantic edge boundaries (click, keyboard, touch).
    pub fn reset_coalesce_burst(&mut self) {
        self.pointer_coalesce_burst = 0;
        self.last_coalesced_pointer = None;
    }

    /// RFC-0055: Compute a deterministic pixel hash of the current composed frame.
    ///
    /// Returns `None` if no frame has been composed. Otherwise returns a hash over the
    /// first 256 pixels of the frame (bounded, deterministic).
    #[must_use = "frame hash is used for skip eligibility; ignoring it makes skip decisions uninformed"]
    pub fn compute_frame_hash(&self) -> Option<u64> {
        let frame = self.last_frame.as_ref()?;
        let mut hash: u64 = 0xcbf29ce484222325;
        let prime: u64 = 0x100000001b3;
        let sample_count = (frame.pixels.len() / 4).min(256);
        for i in 0..sample_count {
            let base = i * 4;
            if base + 4 <= frame.pixels.len() {
                let pixel = u32::from_le_bytes([
                    frame.pixels[base],
                    frame.pixels[base + 1],
                    frame.pixels[base + 2],
                    frame.pixels[base + 3],
                ]);
                hash ^= pixel as u64;
                hash = hash.wrapping_mul(prime);
            }
        }
        Some(hash)
    }

    /// RFC-0055: Try to skip a present tick if no visible change has occurred.
    ///
    /// Returns `Ok(true)` if the present can be skipped safely.
    /// Returns `Ok(false)` if a full present is required.
    /// Returns `Err(IdleCheapBudgetExceeded)` if too many idle-cheap wakeups.
    #[must_use = "skip decision must be checked: true means frame skipped, false means full present needed"]
    pub fn try_no_damage_skip(&mut self) -> Result<bool> {
        let current_hash = self.compute_frame_hash();
        let no_change = match (self.last_frame_hash, current_hash) {
            (Some(prev), Some(curr)) => prev == curr,
            _ => false,
        };
        if no_change && self.no_damage_skips < MAX_NO_DAMAGE_SKIPS {
            self.no_damage_skips = self.no_damage_skips.saturating_add(1);
            self.idle_cheap_wakeups = self.idle_cheap_wakeups.saturating_add(1);
            if self.idle_cheap_wakeups > MAX_IDLE_CHEAP_WAKEUPS {
                return Err(WindowdError::IdleCheapBudgetExceeded);
            }
            return Ok(true);
        }
        // RFC-0055: Forced present resets counters for next 4-of-5 cycle.
        // Whether triggered by frame change or budget exhaustion, a forced
        // present always resets the skip budget.
        self.no_damage_skips = 0;
        self.idle_cheap_wakeups = 0;
        self.last_frame_hash = current_hash;
        Ok(false)
    }

    /// RFC-0055: Coalesce burst counter for telemetry.
    pub const fn pointer_coalesce_burst(&self) -> u64 {
        self.pointer_coalesce_burst
    }

    /// RFC-0055: Last coalesced pointer position (None if no coalesced move or after reset).
    pub const fn last_coalesced_pointer(&self) -> Option<PointerPosition> {
        self.last_coalesced_pointer
    }

    /// RFC-0055: No-damage skip counter for telemetry.
    pub const fn no_damage_skips(&self) -> u64 {
        self.no_damage_skips
    }

    /// RFC-0055: Idle-cheap wakeup counter for telemetry.
    pub const fn idle_cheap_wakeups(&self) -> u64 {
        self.idle_cheap_wakeups
    }

    /// TEST-ONLY: Directly set last_frame_hash to simulate frame identity.
    #[doc(hidden)]
    pub fn set_last_frame_hash_for_tests(&mut self, hash: Option<u64>) {
        self.last_frame_hash = hash;
    }

    /// TEST-ONLY: Directly set idle_cheap_wakeups counter.
    #[doc(hidden)]
    pub fn set_idle_cheap_wakeups_for_tests(&mut self, value: u64) {
        self.idle_cheap_wakeups = value;
    }

    #[must_use = "drain count must be checked; zero means no events were pending"]
    pub fn drain_input_events(
        &mut self,
        caller: CallerCtx,
        surface_id: SurfaceId,
    ) -> Result<usize> {
        let surface = self.surface(surface_id).ok_or(WindowdError::StaleSurfaceId)?;
        if surface.owner != caller.caller_id() {
            return Err(WindowdError::Unauthorized);
        }

        let mut drained = 0usize;
        self.input_events.retain(|event| {
            let keep = event.surface != surface_id;
            if !keep {
                drained = drained.saturating_add(1);
            }
            keep
        });
        Ok(drained)
    }

    pub const fn focused_surface(&self) -> Option<SurfaceId> {
        self.focused_surface
    }

    pub const fn input_enabled(&self) -> bool {
        self.input_enabled
    }

    pub const fn scheduler_enabled(&self) -> bool {
        self.scheduler_enabled
    }

    pub const fn last_scheduled_present(&self) -> Option<ScheduledPresentAck> {
        self.last_scheduled_present
    }

    pub const fn last_pointer_hit(&self) -> Option<SurfaceId> {
        self.last_pointer_hit
    }

    pub const fn pointer_position(&self) -> Option<PointerPosition> {
        self.pointer_position
    }

    pub fn render_visible_input_frame(&mut self) -> Result<Frame> {
        if self.layers.is_empty() {
            return Err(WindowdError::NoCommittedScene);
        }
        let frame = self.compose_frame()?;
        self.last_frame = Some(frame.clone());
        Ok(frame)
    }

    pub fn marker_evidence(&self) -> Result<PresentAck> {
        self.last_present.ok_or(WindowdError::MarkerBeforePresentState)
    }

    pub fn last_frame(&self) -> Option<&Frame> {
        self.last_frame.as_ref()
    }

    fn total_damage_count(&self) -> Result<u16> {
        let mut total: usize = 0;
        for surface in &self.surfaces {
            total =
                total.checked_add(surface.damage.len()).ok_or(WindowdError::ArithmeticOverflow)?;
        }
        u16::try_from(total).map_err(|_| WindowdError::TooManyDamageRects)
    }

    fn compose_frame(&self) -> Result<Frame> {
        let stride = checked_stride(self.config.width)?;
        let len = checked_len(stride, self.config.height)?;
        let mut out = Frame {
            width: self.config.width,
            height: self.config.height,
            stride,
            pixels: vec![0u8; len],
        };
        for layer in &self.layers {
            let surface = self.surface(layer.surface).ok_or(WindowdError::StaleSurfaceId)?;
            blit_surface(&mut out, layer, &surface.buffer)?;
        }
        self.draw_visible_input_affordances(&mut out)?;
        Ok(out)
    }

    fn surface(&self, id: SurfaceId) -> Option<&Surface> {
        self.surfaces.iter().find(|surface| surface.id == id)
    }

    fn surface_mut(&mut self, id: SurfaceId) -> Option<&mut Surface> {
        self.surfaces.iter_mut().find(|surface| surface.id == id)
    }

    fn surface_index(&self, id: SurfaceId) -> Option<usize> {
        self.surfaces.iter().position(|surface| surface.id == id)
    }

    fn signal_fence(&mut self, fence_id: FenceId, present_seq: PresentSeq, coalesced: bool) {
        if let Some(record) = self.fences.iter_mut().find(|record| record.id == fence_id) {
            record.state = FenceState::Signaled { present_seq, coalesced };
        }
    }

    fn hit_test(&self, x: i32, y: i32) -> Option<SurfaceId> {
        for layer in self.layers.iter().rev() {
            let Some(surface) = self.surface(layer.surface) else {
                continue;
            };
            if layer.contains_point(surface.buffer.width, surface.buffer.height, x, y) {
                return Some(surface.id);
            }
        }
        None
    }

    fn validate_pointer_position(&self, x: i32, y: i32) -> Result<PointerPosition> {
        let width =
            i32::try_from(self.config.width).map_err(|_| WindowdError::InvalidDimensions)?;
        let height =
            i32::try_from(self.config.height).map_err(|_| WindowdError::InvalidDimensions)?;
        if x < 0 || y < 0 || x >= width || y >= height {
            return Err(WindowdError::InvalidPointerPosition);
        }
        Ok(PointerPosition { x, y })
    }

    fn ensure_input_capacity(&self) -> Result<()> {
        if self.input_events.len() >= MAX_INPUT_EVENTS {
            return Err(WindowdError::InputEventQueueFull);
        }
        Ok(())
    }

    fn draw_visible_input_affordances(&self, frame: &mut Frame) -> Result<()> {
        if let Some(hovered) = self.last_pointer_hit {
            for layer in self.layers.iter().rev() {
                if layer.surface != hovered {
                    continue;
                }
                if let Some(surface) = self.surface(layer.surface) {
                    draw_hover_border(frame, layer, surface.buffer.width, surface.buffer.height)?;
                }
                break;
            }
        }
        if let Some(focused) = self.focused_surface {
            for layer in self.layers.iter().rev() {
                if layer.surface != focused {
                    continue;
                }
                if let Some(surface) = self.surface(layer.surface) {
                    draw_focus_border(frame, layer, surface.buffer.width, surface.buffer.height)?;
                }
                break;
            }
        }
        if let Some(position) = self.pointer_position {
            draw_cursor(frame, position)?;
        }
        Ok(())
    }

    fn push_input_delivery(
        &mut self,
        surface: SurfaceId,
        kind: InputEventKind,
    ) -> Result<InputDelivery> {
        if self.input_events.len() >= MAX_INPUT_EVENTS {
            return Err(WindowdError::InputEventQueueFull);
        }
        let seq = InputSeq::new(self.next_input_seq);
        self.next_input_seq =
            self.next_input_seq.checked_add(1).ok_or(WindowdError::ArithmeticOverflow)?;
        let delivery = InputDelivery { seq, surface, kind };
        self.input_events.push(delivery);
        Ok(delivery)
    }
}

fn draw_hover_border(frame: &mut Frame, layer: &Layer, width: u32, height: u32) -> Result<()> {
    draw_border(frame, layer, width, height, VISIBLE_HOVER_BGRA)
}

fn draw_focus_border(frame: &mut Frame, layer: &Layer, width: u32, height: u32) -> Result<()> {
    draw_border(frame, layer, width, height, VISIBLE_FOCUS_BGRA)
}

fn draw_border(
    frame: &mut Frame,
    layer: &Layer,
    width: u32,
    height: u32,
    bgra: [u8; 4],
) -> Result<()> {
    let width = i32::try_from(width).map_err(|_| WindowdError::InvalidDimensions)?;
    let height = i32::try_from(height).map_err(|_| WindowdError::InvalidDimensions)?;
    for dx in 0..width {
        put_pixel(frame, layer.x.saturating_add(dx), layer.y, bgra)?;
        put_pixel(
            frame,
            layer.x.saturating_add(dx),
            layer.y.saturating_add(height.saturating_sub(1)),
            bgra,
        )?;
    }
    for dy in 0..height {
        put_pixel(frame, layer.x, layer.y.saturating_add(dy), bgra)?;
        put_pixel(
            frame,
            layer.x.saturating_add(width.saturating_sub(1)),
            layer.y.saturating_add(dy),
            bgra,
        )?;
    }
    Ok(())
}

fn draw_cursor(frame: &mut Frame, position: PointerPosition) -> Result<()> {
    for dy in 0..3 {
        for dx in 0..3 {
            put_pixel(
                frame,
                position.x.saturating_add(dx),
                position.y.saturating_add(dy),
                VISIBLE_CURSOR_BGRA,
            )?;
        }
    }
    Ok(())
}

fn put_pixel(frame: &mut Frame, x: i32, y: i32, bgra: [u8; 4]) -> Result<()> {
    if x < 0 || y < 0 {
        return Ok(());
    }
    let x = u32::try_from(x).map_err(|_| WindowdError::ArithmeticOverflow)?;
    let y = u32::try_from(y).map_err(|_| WindowdError::ArithmeticOverflow)?;
    if x >= frame.width || y >= frame.height {
        return Ok(());
    }
    let idx = (y as usize)
        .checked_mul(frame.stride as usize)
        .and_then(|base| base.checked_add((x as usize).checked_mul(4)?))
        .ok_or(WindowdError::ArithmeticOverflow)?;
    frame.pixels[idx..idx + 4].copy_from_slice(&bgra);
    Ok(())
}

#[cfg(test)]
mod rfc0055_tests {
    use super::*;

    fn new_test_server() -> WindowServer {
        WindowServer::new(WindowdConfig::default()).expect("test server")
    }

    fn solid_test_buffer(caller: crate::CallerCtx, width: u32, height: u32) -> SurfaceBuffer {
        SurfaceBuffer::solid(caller, 1, width, height, [0xff, 0xff, 0xff, 0xff])
            .expect("solid test buffer")
    }

    fn solid_damage_rect() -> Rect {
        Rect { x: 0, y: 0, width: 1, height: 1 }
    }

    fn setup_scene(server: &mut WindowServer) {
        let caller = crate::CallerCtx::system();
        let buf = solid_test_buffer(caller, 64, 48);
        let buf2 = solid_test_buffer(caller, 64, 48);
        let sid = server.create_surface(caller, buf).expect("create surface");
        let layer = Layer { surface: sid, x: 0, y: 0, z: 0 };
        server.commit_scene(caller, CommitSeq::new(1), &[layer]).expect("commit scene");
        let damage = solid_damage_rect();
        server.queue_buffer(caller, sid, buf2, &[damage]).expect("queue buffer");
    }

    #[test]
    fn destroy_surface_frees_and_is_owner_gated() {
        let mut server = new_test_server();
        let owner = crate::CallerCtx::system();
        let buf = solid_test_buffer(owner, 64, 48);
        let sid = server.create_surface(owner, buf).expect("create surface");

        // A different caller cannot destroy someone else's surface.
        let intruder = crate::CallerCtx::from_service_metadata(0xBEEF);
        assert_eq!(server.destroy_surface(intruder, sid), Err(WindowdError::Unauthorized));

        // The owner frees it; a second free is a stale id (reclaimed).
        server.destroy_surface(owner, sid).expect("destroy");
        assert_eq!(server.destroy_surface(owner, sid), Err(WindowdError::StaleSurfaceId));
    }

    #[test]
    fn reject_coalesce_when_fastpath_disabled() {
        let mut server = new_test_server();
        assert_eq!(server.try_coalesce_pointer_move(10, 20), Err(WindowdError::FastPathDisabled));
    }

    #[test]
    fn coalesce_pointer_move_within_budget() {
        let mut server = new_test_server();
        server.enable_fastpath();
        for i in 0..MAX_POINTER_COALESCE_BURST {
            assert!(server.try_coalesce_pointer_move(i as i32, i as i32).unwrap());
        }
    }

    #[test]
    fn reject_coalesce_when_burst_exceeded() {
        let mut server = new_test_server();
        server.enable_fastpath();
        for i in 0..MAX_POINTER_COALESCE_BURST {
            assert!(server.try_coalesce_pointer_move(i as i32, i as i32).unwrap());
        }
        assert_eq!(
            server.try_coalesce_pointer_move(99, 99),
            Err(WindowdError::CoalesceBurstExceeded)
        );
    }

    #[test]
    fn reset_coalesce_burst_clears_counter() {
        let mut server = new_test_server();
        server.enable_fastpath();
        for i in 0..MAX_POINTER_COALESCE_BURST {
            server.try_coalesce_pointer_move(i as i32, i as i32).unwrap();
        }
        assert_eq!(
            server.try_coalesce_pointer_move(99, 99),
            Err(WindowdError::CoalesceBurstExceeded)
        );
        server.reset_coalesce_burst();
        assert_eq!(server.pointer_coalesce_burst(), 0);
        assert!(server.try_coalesce_pointer_move(1, 1).unwrap());
    }

    #[test]
    fn compute_frame_hash_is_deterministic() {
        let mut server = new_test_server();
        setup_scene(&mut server);
        let _present = server.present_tick().expect("present tick");
        let hash1 = server.compute_frame_hash();
        let hash2 = server.compute_frame_hash();
        assert_eq!(hash1, hash2, "frame hash must be deterministic");
    }

    #[test]
    fn no_damage_skip_within_budget() {
        let mut server = new_test_server();
        setup_scene(&mut server);
        // First present – establishes baseline hash
        let _present = server.present_tick().expect("first present");
        // Force hash update for test (damage cleared; reuse same hash)
        server.last_frame_hash = server.compute_frame_hash();
        // Subsequent ticks with same frame should be skippable
        for i in 0..MAX_NO_DAMAGE_SKIPS {
            assert!(server.try_no_damage_skip().unwrap(), "skip {} should succeed", i);
        }
    }

    #[test]
    fn no_damage_skip_forced_after_budget() {
        let mut server = new_test_server();
        setup_scene(&mut server);
        let _present = server.present_tick().expect("first present");
        server.last_frame_hash = server.compute_frame_hash();
        // Exhaust skip budget
        for _ in 0..MAX_NO_DAMAGE_SKIPS {
            assert!(server.try_no_damage_skip().unwrap());
        }
        // Next call should force a present (return false)
        assert!(!server.try_no_damage_skip().unwrap());
    }

    #[test]
    fn no_damage_skip_resets_on_change() {
        let mut server = new_test_server();
        setup_scene(&mut server);
        let _present = server.present_tick().expect("first present");
        server.last_frame_hash = server.compute_frame_hash();
        // Skip once
        assert!(server.try_no_damage_skip().unwrap());
        // Simulate frame change
        server.last_frame_hash = Some(0xDEAD_BEEF_CAFE_BABE);
        // Should now require present (hash mismatch)
        assert!(!server.try_no_damage_skip().unwrap());
    }

    #[test]
    fn idle_cheap_exceeded_error() {
        let mut server = new_test_server();
        setup_scene(&mut server);
        let _present = server.present_tick().expect("first present");
        // Establish hash match so no_change is true
        server.last_frame_hash = server.compute_frame_hash();
        // Push idle_cheap_wakeups to threshold, then one more skip will exceed
        server.idle_cheap_wakeups = MAX_IDLE_CHEAP_WAKEUPS;
        // First skip within budget should succeed (no_change true, no_damage_skips < 4)
        // But idle_cheap_wakeups will become MAX+1 which exceeds MAX, triggering error
        assert_eq!(server.try_no_damage_skip(), Err(WindowdError::IdleCheapBudgetExceeded));
    }

    #[test]
    fn semantic_edge_reject_pointer_down_preserves_click() {
        // RFC-0055: Click events must NOT be collapsed.
        // The windowd routing path for PointerDown creates a distinct input event.
        let mut server = new_test_server();
        setup_scene(&mut server);
        // Route a pointer move, then a pointer down:
        let move_delivery = server.route_pointer_move(10, 10).expect("pointer move");
        assert!(matches!(move_delivery.kind, InputEventKind::PointerMove { .. }));
        let click_delivery = server.route_pointer_down(10, 10).expect("pointer down");
        assert!(matches!(click_delivery.kind, InputEventKind::PointerDown));
        // Both must be present in input queue (not coalesced away)
        let events = server
            .take_input_events(crate::CallerCtx::system(), SurfaceId::new(1))
            .expect("drain surface");
        assert_eq!(events.len(), 2, "click must not be coalesced away");
        assert!(matches!(events[0].kind, InputEventKind::PointerMove { .. }));
        assert!(matches!(events[1].kind, InputEventKind::PointerDown));
    }

    #[test]
    fn semantic_edge_reject_keyboard_preserves_key() {
        // RFC-0055: Keyboard edges must NOT be collapsed.
        let mut server = new_test_server();
        setup_scene(&mut server);
        // Establish focus via pointer-down
        let _ = server.route_pointer_down(10, 10).expect("pointer down");
        // Send keyboard event
        let key_delivery = server.route_keyboard(0x04).expect("keyboard");
        assert!(matches!(key_delivery.kind, InputEventKind::Keyboard { key_code: 0x04 }));
        let events = server
            .take_input_events(crate::CallerCtx::system(), SurfaceId::new(1))
            .expect("drain surface");
        assert_eq!(events.len(), 2, "key must not be coalesced away");
        assert!(matches!(events[1].kind, InputEventKind::Keyboard { key_code: 0x04 }));
    }

    #[test]
    fn pointer_coalesce_counters_exposed_for_telemetry() {
        let mut server = new_test_server();
        assert_eq!(server.pointer_coalesce_burst(), 0);
        assert_eq!(server.no_damage_skips(), 0);
        assert_eq!(server.idle_cheap_wakeups(), 0);
        server.enable_fastpath();
        for i in 0..3 {
            server.try_coalesce_pointer_move(i, i).unwrap();
        }
        assert_eq!(server.pointer_coalesce_burst(), 3);
    }

    #[test]
    fn fastpath_state_is_queries() {
        let mut server = new_test_server();
        assert!(!server.fastpath_enabled());
        server.enable_fastpath();
        assert!(server.fastpath_enabled());
    }
}
