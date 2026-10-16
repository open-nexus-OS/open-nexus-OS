// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What a client of the GPU can do — `GfxDevice`, the v2 backend contract of
//! RFC-0101 §1–§7: capabilities before the first context; a context per client with its own
//! address space; buffers with a cache mode by type, destroyed by their client; ranges of them
//! bound (`Mapping`); one batch per submit, bounded, waiting on and signalling points of the
//! one timeline; GPU code as client data with a declared target. The client's identity is
//! passed in by the server that derived it from the kernel (`sender_service_id`) — never read
//! from a payload. [`scanout`] is §8, the
//! contract between the compositor and the display; [`cpu_mock`] implements this trait on
//! the host as the oracle every later package is proven against.
//!
//! The v1 trait (`backend::traits::GfxBackend`) and the boolean `core::fence::Fence` stay
//! until RFC-0101 Phase 3 deletes them with the retired-name gate.
//! OWNERS: @gpu @ui @runtime
//! STATUS: Functional (contract; the device implementation is RFC-0101 Phase 3)
//! API_STABILITY: Unstable until RFC-0101 Phase 3
//! TEST_COVERAGE: `cpu_mock` and `scanout` unit tests — the reject matrix of RFC-0101 §9
//! RFC: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md

pub mod cpu_mock;
pub mod scanout;

pub use nexus_driverkit::gpu::{
    Access, BatchError, CacheMode, Capabilities, CodeTarget, GpuVa, Priority, QueueKind,
    QueueKinds, TimelinePoint, VmError, MAX_BATCH_COMMANDS,
};

use crate::command::buffer::Command;

/// The client a context or buffer belongs to: the kernel's `sender_service_id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClientId(pub u64);

/// A context, as the device numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContextId(pub u32);

/// A buffer object, as the device numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoId(pub u32);

/// A range of a buffer at an address of a context's space (§3): `len` bytes of `bo` from
/// `offset`, at `va` — a client binds ranges of a few large buffers, as every production GPU
/// stack does, and one buffer may be bound in several ranges and contexts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub bo: BoId,
    pub offset: u64,
    pub len: u64,
    pub va: GpuVa,
    pub access: Access,
}

impl Mapping {
    /// The first `len` bytes of `bo` at `va`.
    pub const fn whole(bo: BoId, len: u64, va: GpuVa, access: Access) -> Self {
        Self { bo, offset: 0, len, va, access }
    }
}

/// GPU code as client data (§7): finished bytes for a declared target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shader<'a> {
    pub target: CodeTarget,
    /// The entry point's offset into `bytes`.
    pub entry: u32,
    pub bytes: &'a [u8],
}

/// What a batch carries: the tree's one command vocabulary, or GPU code at a bound address.
#[derive(Clone, Copy, Debug)]
pub enum Stream<'a> {
    Commands(&'a [Command]),
    Code {
        shader: Shader<'a>,
        at: GpuVa,
    },
    /// One transfer job's register stream in the open driver's definition (RFC-0101 §8c); the
    /// programs it points at are GPU code already bound in the context. `single_core` is the
    /// job's one flag.
    Transfer {
        stream: &'a [u8],
        single_core: bool,
    },
    /// One render pass: the geometry, partial-render fragment and fragment jobs' register
    /// streams with their submit flags (RFC-0101 §8d); the dataset is the device's.
    Render(RenderStreams<'a>),
}

/// The three register streams of one render pass and their submit flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderStreams<'a> {
    pub geom: &'a [u8],
    pub geom_flags: u32,
    pub frag_pr: &'a [u8],
    pub frag_pr_flags: u32,
    pub frag: &'a [u8],
    pub frag_flags: u32,
}

impl Stream<'_> {
    /// Commands in the stream, for the batch bound (a binary or a pass counts as one).
    pub fn commands(&self) -> usize {
        match self {
            Stream::Commands(c) => c.len(),
            Stream::Code { .. } | Stream::Transfer { .. } | Stream::Render(_) => 1,
        }
    }
}

/// The surface a batch draws into: a bound buffer and the picture's geometry in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTarget {
    pub va: GpuVa,
    pub width: u32,
    pub height: u32,
}

impl RenderTarget {
    /// Bytes of an RGBA8 picture of this geometry.
    pub fn bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 4
    }
}

/// One submit: one IPC round trip, one kernel crossing (§4).
#[derive(Clone, Copy, Debug)]
pub struct Batch<'a> {
    pub target: RenderTarget,
    pub stream: Stream<'a>,
    /// Points the batch waits for before it runs.
    pub waits: &'a [TimelinePoint],
    /// The point the batch signals when it is done.
    pub signal: TimelinePoint,
}

/// Why the device refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceError {
    /// The context or buffer belongs to another client.
    NotOwner,
    /// No such context, or it was destroyed.
    NoSuchContext,
    NoSuchBuffer,
    /// The device's context or memory bound is reached.
    Exhausted,
    /// The address space refused (binding or reference).
    Vm(VmError),
    /// A batch above the device's bound.
    Batch(BatchError),
    /// A queue kind the device does not execute — refused by name.
    QueueUnsupported(QueueKind),
    /// GPU code for another device.
    CodeTargetMismatch {
        declared: CodeTarget,
        device: CodeTarget,
    },
    /// GPU code on a device that does not execute it (the mock, Phase 2's device).
    CodeUnsupported,
    /// A wait the mock cannot defer: the point is not reached.
    WouldBlock(TimelinePoint),
    /// The tree's command vocabulary on a device that executes GPU code only (the hardware).
    CommandsUnsupported,
    /// A cache mode the device does not offer yet — refused by name (M2 brings `Uncached`).
    CacheModeUnsupported(CacheMode),
    /// A job stream the device's decoder refused (length, padding, extension).
    MalformedStream,
    /// The device did not answer (a flush or a kick without return); named on the console.
    Lost,
    /// A render pass before the render context, free lists and dataset were set up (§8d).
    NoRenderTarget,
    /// A buffer still bound in a context: unbind it before it is destroyed (§3).
    BufferBound,
}

impl From<VmError> for DeviceError {
    fn from(e: VmError) -> Self {
        Self::Vm(e)
    }
}

impl From<BatchError> for DeviceError {
    fn from(e: BatchError) -> Self {
        Self::Batch(e)
    }
}

/// The v2 device contract.
pub trait GfxDevice {
    /// What this device can do, before the first context.
    fn capabilities(&self) -> Capabilities;
    /// A context for `client` with its own address space.
    fn create_context(
        &mut self,
        client: ClientId,
        priority: Priority,
    ) -> Result<ContextId, DeviceError>;
    /// Destroys the context: its bindings and the device's objects for it; the client's buffers
    /// stay the client's.
    fn destroy_context(&mut self, client: ClientId, ctx: ContextId) -> Result<(), DeviceError>;
    /// A buffer of `bytes` (rounded up to the page) with its cache mode, owned by `client`.
    fn create_bo(
        &mut self,
        client: ClientId,
        bytes: usize,
        mode: CacheMode,
    ) -> Result<BoId, DeviceError>;
    /// Destroys a buffer of `client`'s; refused while a context binds it.
    fn destroy_bo(&mut self, client: ClientId, bo: BoId) -> Result<(), DeviceError>;
    /// Binds the range `m` of a buffer in `ctx`; both must belong to `client`.
    fn vm_bind(&mut self, client: ClientId, ctx: ContextId, m: Mapping) -> Result<(), DeviceError>;
    /// Unbinds the binding that starts at `va`.
    fn vm_unbind(&mut self, client: ClientId, ctx: ContextId, va: GpuVa)
        -> Result<(), DeviceError>;
    /// One batch on one queue; the point it will signal.
    fn submit(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        kind: QueueKind,
        batch: &Batch<'_>,
    ) -> Result<TimelinePoint, DeviceError>;
    /// Whether the device's timeline reached `point`.
    fn reached(&self, point: TimelinePoint) -> bool;
    /// Cancels the context's in-flight work; its space is kept (§6).
    fn reset_context(&mut self, client: ClientId, ctx: ContextId) -> Result<(), DeviceError>;
}
