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
pub const VISIBLE_BOOTSTRAP_HZ: u16 = 60;
pub const VISIBLE_BOOTSTRAP_FORMAT: PixelFormat = PixelFormat::Bgra8888;
const MAX_BACK_BUFFERS_PER_SURFACE: usize = 2;
const MAX_FENCES: usize = 64;
const MAX_INPUT_EVENTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    PointerDown,
    Keyboard { key_code: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputDelivery {
    pub seq: InputSeq,
    pub surface: SurfaceId,
    pub kind: InputEventKind,
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
            let Some(pending) = surface.pending_present.take() else {
                continue;
            };
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
        }
        if damage_count == 0 {
            return Ok(None);
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

    pub fn route_pointer_down(&mut self, x: i32, y: i32) -> Result<InputDelivery> {
        let surface_id = self.hit_test(x, y).ok_or(WindowdError::StaleSurfaceId)?;
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
        self.input_enabled = true;
        self.push_input_delivery(surface_id, InputEventKind::Keyboard { key_code })
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
