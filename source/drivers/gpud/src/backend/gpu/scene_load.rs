// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A scene bundle into a context (TASK-0215 5c-3 P2a, RFC-0101 §3/§8c): every buffer of the
//! bundle bound at its captured address — packed by the open driver's flags into one buffer per
//! class (GPU cache allowed or not, parameter manager's or not) and bound in ranges, the bytes
//! the bundle carries written once, the texture made by its formula; a large buffer the bundle
//! carries as zeros (the partial render's scratch) the GPU's alone; the target a plane of the
//! ring or a buffer the caller made. A scene so costs a context at most four regions of gpud's
//! address space instead of one a buffer (the render bundle took 36 of the 64), and is given
//! back whole ([`Loaded::release`]) once its context is gone.

use nexus_driverkit::gpu::{Access, CacheMode, GpuVa};
use nexus_gfx::backend::gpu::scene::{self, Kind, Scene};
use nexus_gfx::backend::v2::{BoId, ClientId, ContextId, DeviceError, GfxDevice, Mapping};

use super::device_v2::Device;

/// A buffer the bundle carries as zeros from this size on is the GPU's alone.
const GPU_ONLY_FROM: u64 = 1 << 20;
/// The classes: the flag pair, CPU-mapped (0..4) or the GPU's alone (4..8).
const CLASSES: usize = 8;
/// Ranges a scene binds (the render bundle has 37 buffers).
const MAX_RANGES: usize = 48;

/// What backs the scene's target.
#[derive(Clone, Copy, Debug)]
pub(super) enum TargetMem {
    /// A plane of the ring at its bus address (adopted by the loader, given back with it).
    Plane { bus: u64 },
    /// A buffer of the caller's, bound whole.
    Buffer(BoId),
    /// Nothing at the captured target's address: a context that names every pass's target
    /// itself (windowd's, 5c-3 P3) — a pass writing the captured one faults there, and the
    /// fault stays in that context (RFC-0101 §6).
    Unbound,
}

/// The texture's formula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Texel {
    /// `Texture::CheckerV1`.
    Checker,
    /// The same with red and blue exchanged: another context's texture at the same address.
    CheckerSwapped,
}

/// A scene's buffers in a context.
pub(crate) struct Loaded {
    pools: [Option<BoId>; CLASSES],
    /// The adopted plane.
    plane: Option<BoId>,
    /// The target's address and bytes.
    target: Option<(GpuVa, u64)>,
    ranges: [Option<GpuVa>; MAX_RANGES],
}

fn class(buf: &scene::Buffer<'_>) -> usize {
    let flags = usize::from(buf.device_cached()) | (usize::from(buf.pm_fw_protect()) << 1);
    let zero = buf.kind == Kind::Data && buf.runs().next().is_none();
    if zero && buf.len >= GPU_ONLY_FROM {
        4 + flags
    } else {
        flags
    }
}

impl Loaded {
    /// A fresh plane under the target's address: the old plane unbound and given back.
    pub(super) fn retarget(
        &mut self,
        dev: &mut Device,
        client: ClientId,
        ctx: ContextId,
        bus: u64,
    ) -> Result<(), DeviceError> {
        let (va, len) = self.target.ok_or(DeviceError::NoSuchBuffer)?;
        dev.vm_unbind(client, ctx, va)?;
        if let Some(old) = self.plane.take() {
            dev.destroy_bo(client, old)?;
        }
        let bo = dev.adopt_plane(client, bus, len as usize)?;
        self.plane = Some(bo);
        dev.vm_bind(client, ctx, Mapping::whole(bo, len, va, Access::ReadWrite))
    }

    /// One of the scene's buffers (a refusal test names it in another context).
    pub(super) fn a_buffer(&self) -> Option<BoId> {
        self.pools.iter().flatten().next().copied()
    }

    /// Unbinds what is still bound in `ctx` (nothing once the context is destroyed) and
    /// destroys the scene's buffers.
    pub(super) fn release(&self, dev: &mut Device, client: ClientId, ctx: ContextId) {
        for va in self.ranges.iter().flatten() {
            let _ = dev.vm_unbind(client, ctx, *va);
        }
        for bo in self.pools.iter().chain(core::iter::once(&self.plane)).flatten() {
            let _ = dev.destroy_bo(client, *bo);
        }
    }
}

/// Loads `scene` into `client`'s context `ctx`: its target backed by `target`, its texture by
/// `texel`. On a refusal everything the loader took is given back.
pub(super) fn load(
    dev: &mut Device,
    client: ClientId,
    ctx: ContextId,
    scene: &Scene<'_>,
    target: TargetMem,
    texel: Texel,
) -> Result<Loaded, alloc::string::String> {
    let mut loaded =
        Loaded { pools: [None; CLASSES], plane: None, target: None, ranges: [None; MAX_RANGES] };
    match fill_and_bind(dev, client, ctx, scene, target, texel, &mut loaded) {
        Ok(()) => Ok(loaded),
        Err(e) => {
            loaded.release(dev, client, ctx);
            Err(e)
        }
    }
}

fn fill_and_bind(
    dev: &mut Device,
    client: ClientId,
    ctx: ContextId,
    scene: &Scene<'_>,
    target: TargetMem,
    texel: Texel,
    loaded: &mut Loaded,
) -> Result<(), alloc::string::String> {
    // Each class's size, then one buffer per class with its flags.
    let mut size = [0u64; CLASSES];
    for buf in scene.buffers().filter(|b| b.kind != Kind::Target) {
        size[class(&buf)] += buf.len;
    }
    for (c, &len) in size.iter().enumerate().filter(|(_, &len)| len > 0) {
        let mode = if c >= 4 { CacheMode::GpuPrivate } else { CacheMode::CpuCached };
        let bo = dev
            .create_bo(client, len as usize, mode)
            .map_err(|e| alloc::format!("create_bo class {c} {len:#x} {e:?}"))?;
        loaded.pools[c] = Some(bo);
        dev.set_bo_flags(client, bo, c & 1 != 0, c & 2 != 0)
            .map_err(|e| alloc::format!("flags class {c} {e:?}"))?;
    }
    // The bytes into the CPU-mapped buffers, each cleaned once (CPU → device).
    let mut at = [0u64; CLASSES];
    for buf in scene.buffers().filter(|b| b.kind != Kind::Target) {
        let c = class(&buf);
        let off = at[c] as usize;
        at[c] += buf.len;
        let Some(bo) = loaded.pools[c] else { continue };
        if c >= 4 {
            continue;
        }
        let bytes = dev.bo_bytes_mut(client, bo).ok_or("no cpu view")?;
        let dst = bytes.get_mut(off..off + buf.len as usize).ok_or("a buffer past its class")?;
        match buf.kind {
            Kind::Texture => {
                scene::checker_v1(256, 256, 1024, dst);
                if texel == Texel::CheckerSwapped {
                    for px in dst.chunks_exact_mut(4) {
                        px.swap(0, 2);
                    }
                }
            }
            _ => {
                for run in buf.runs() {
                    let r = run.off as usize;
                    if let Some(d) = dst.get_mut(r..r + run.bytes.len()) {
                        d.copy_from_slice(run.bytes);
                    }
                }
            }
        }
    }
    for bo in loaded.pools[..4].iter().flatten() {
        dev.clean(client, *bo);
    }
    // The ranges at their captured addresses; the target last.
    let mut at = [0u64; CLASSES];
    for (n, buf) in scene.buffers().enumerate() {
        let (bo, offset, access) = match buf.kind {
            Kind::Target => {
                let bo = match target {
                    TargetMem::Plane { bus } => {
                        let bo = dev
                            .adopt_plane(client, bus, buf.len as usize)
                            .map_err(|e| alloc::format!("adopt {e:?}"))?;
                        loaded.plane = Some(bo);
                        bo
                    }
                    TargetMem::Buffer(bo) => bo,
                    TargetMem::Unbound => continue,
                };
                loaded.target = Some((GpuVa(buf.va), buf.len));
                (bo, 0, Access::ReadWrite)
            }
            _ => {
                let c = class(&buf);
                let offset = at[c];
                at[c] += buf.len;
                let bo = loaded.pools[c].ok_or("a class without its buffer")?;
                // The texture is only sampled; the driver's data buffers are written by the GPU
                // too — region headers, macrotile lists, tail pointers, context state, the free
                // lists (cycle 3 of 2026-10-14: a write fault at the first region header,
                // `fault=0xdbf0000000`, with every data buffer bound read-only).
                let access =
                    if buf.kind == Kind::Texture { Access::ReadOnly } else { Access::ReadWrite };
                (bo, offset, access)
            }
        };
        let va = GpuVa(buf.va);
        let slot = loaded.ranges.get_mut(n).ok_or("more ranges than a scene binds")?;
        let m = Mapping { bo, offset, len: buf.len, va, access };
        dev.vm_bind(client, ctx, m).map_err(|e| alloc::format!("bind {:#x} {e:?}", buf.va))?;
        *slot = Some(va);
    }
    Ok(())
}
