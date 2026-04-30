// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `windowd` state machine for surfaces, scene commits, and present sequencing.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No direct tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::vec;
use alloc::vec::Vec;

use crate::buffer::{validate_buffer, PixelFormat, SurfaceBuffer};
use crate::error::{Result, WindowdError};
use crate::frame::{blit_surface, Frame, Layer};
use crate::geometry::{
    checked_len, checked_stride, validate_damage, validate_dimensions, Rect, MAX_LAYERS,
    MAX_SURFACES,
};
use crate::ids::{CallerCtx, CommitSeq, PresentSeq, SurfaceId};

pub(crate) const DEFAULT_WIDTH: u32 = 64;
pub(crate) const DEFAULT_HEIGHT: u32 = 48;
pub(crate) const DEFAULT_HZ: u16 = 60;
pub const VISIBLE_BOOTSTRAP_WIDTH: u32 = 1280;
pub const VISIBLE_BOOTSTRAP_HEIGHT: u32 = 800;
pub const VISIBLE_BOOTSTRAP_HZ: u16 = 60;
pub const VISIBLE_BOOTSTRAP_FORMAT: PixelFormat = PixelFormat::Bgra8888;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentAck {
    pub seq: PresentSeq,
    pub damage_rects: u16,
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
        self.surfaces.push(Surface { id, owner: caller.caller_id(), buffer, damage: Vec::new() });
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
}
