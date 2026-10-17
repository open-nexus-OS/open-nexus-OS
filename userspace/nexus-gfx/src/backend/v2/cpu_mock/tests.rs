// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: unit tests of the CPU mock device (RFC-0101 §9's reject matrix): a batch lands in
//! the bound target and signals its point; buffers, mappings, contexts and queues refused by
//! name — another client's buffer, a mapping past the buffer, a bound buffer destroyed, an
//! unbound reference, a queue not built, an oversized batch, a destroyed or lost context, GPU
//! code for another target, a wait the mock cannot defer; two contexts reaching their own
//! memory through the same address.
//! OWNERS: @gpu @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file (`cargo test -p nexus-gfx --lib backend::v2::cpu_mock`).

use super::*;
use crate::backend::v2::{Access, RenderTarget, Shader};
use crate::command::buffer::{Command, RgbaColor};
use crate::core::types::TileRect;
use crate::shader::library::{feature, LibraryError};
use nexus_driverkit::gpu::BatchError;

const WINDOWD: ClientId = ClientId(0x600e3345cc7104bb);
const APP: ClientId = ClientId(0xdd8c136b3d65f5dc);
const VA: GpuVa = GpuVa(0x10_0000);

fn target(va: GpuVa) -> RenderTarget {
    RenderTarget { va, width: 8, height: 8 }
}

fn fill() -> Vec<Command> {
    vec![Command::DrawTiles {
        tiles: vec![TileRect { x: 1, y: 1, width: 2, height: 1 }],
        color: RgbaColor::new(1, 2, 3, 255),
    }]
}

fn batch<'a>(va: GpuVa, commands: &'a [Command], signal: u64) -> Batch<'a> {
    Batch {
        target: target(va),
        stream: Stream::Commands(commands),
        waits: &[],
        signal: TimelinePoint(signal),
    }
}

fn bound_device() -> (CpuMockDevice, ContextId, BoId) {
    let mut dev = CpuMockDevice::phase2();
    let ctx = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    let bo = dev.create_bo(WINDOWD, 8 * 8 * 4, CacheMode::CpuCached).unwrap();
    dev.vm_bind(WINDOWD, ctx, Mapping::whole(bo, 4096, VA, Access::ReadWrite)).unwrap();
    (dev, ctx, bo)
}

#[test]
fn a_batch_lands_in_the_bound_target_and_signals_its_point() {
    let (mut dev, ctx, bo) = bound_device();
    let commands = fill();
    assert!(!dev.reached(TimelinePoint(1)));
    let point = dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(VA, &commands, 1)).unwrap();
    assert_eq!(point, TimelinePoint(1));
    assert!(dev.reached(point));
    let bytes = dev.bo_bytes(WINDOWD, bo).unwrap();
    assert_eq!(&bytes[(8 + 1) * 4..(8 + 3) * 4], &[1, 2, 3, 255, 1, 2, 3, 255]);
    assert_eq!(&bytes[0..4], &[0, 0, 0, 0]);
    assert_eq!(dev.bo_mode(bo), Ok(CacheMode::CpuCached));
    assert_eq!(dev.batches(), 1);
    assert_eq!(dev.bo_bytes(APP, bo), Err(DeviceError::NotOwner));
    // The timeline never moves backwards: an older point is a no-op, still returned.
    assert_eq!(
        dev.submit(WINDOWD, ctx, QueueKind::Transfer, &batch(VA, &commands, 0)),
        Ok(TimelinePoint(0))
    );
    assert!(dev.reached(TimelinePoint(1)));
}

#[test]
fn test_reject_bind_bo_of_another_client() {
    let mut dev = CpuMockDevice::phase2();
    let app = dev.create_context(APP, Priority::Normal).unwrap();
    let bo = dev.create_bo(WINDOWD, 4096, CacheMode::GpuPrivate).unwrap();
    let read = |bo| Mapping::whole(bo, 4096, VA, Access::ReadOnly);
    assert_eq!(dev.vm_bind(APP, app, read(bo)), Err(DeviceError::NotOwner));
    let own = dev.create_bo(APP, 4096, CacheMode::Uncached).unwrap();
    let other = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    assert_eq!(dev.vm_bind(APP, other, read(own)), Err(DeviceError::NotOwner));
    assert_eq!(dev.vm_unbind(WINDOWD, app, VA), Err(DeviceError::NotOwner));
    assert_eq!(dev.destroy_bo(APP, bo), Err(DeviceError::NotOwner));
    assert_eq!(dev.vm_bind(APP, app, read(own)), Ok(()));
}

#[test]
fn test_reject_mapping_past_the_buffer() {
    let mut dev = CpuMockDevice::phase2();
    let ctx = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    let bo = dev.create_bo(WINDOWD, 2 * 4096, CacheMode::CpuCached).unwrap();
    let range = |offset, len| Mapping { bo, offset, len, va: VA, access: Access::ReadWrite };
    let past = Err(DeviceError::Vm(VmError::OutOfRange));
    assert_eq!(dev.vm_bind(WINDOWD, ctx, range(4096, 2 * 4096)), past);
    assert_eq!(dev.vm_bind(WINDOWD, ctx, range(2 * 4096, 4096)), past);
    assert_eq!(dev.vm_bind(WINDOWD, ctx, range(u64::MAX - 0xfff, 4096)), past);
    assert_eq!(
        dev.vm_bind(WINDOWD, ctx, range(2048, 4096)),
        Err(DeviceError::Vm(VmError::Unaligned)),
        "a range starts on a page"
    );
    assert_eq!(dev.vm_bind(WINDOWD, ctx, range(4096, 4096)), Ok(()));
}

#[test]
fn test_reject_destroy_bound_bo() {
    let (mut dev, ctx, bo) = bound_device();
    assert_eq!(dev.destroy_bo(WINDOWD, bo), Err(DeviceError::BufferBound));
    assert_eq!(dev.destroy_bo(APP, bo), Err(DeviceError::NotOwner));
    dev.vm_unbind(WINDOWD, ctx, VA).unwrap();
    assert_eq!(dev.destroy_bo(WINDOWD, bo), Ok(()));
    assert_eq!(dev.destroy_bo(WINDOWD, bo), Err(DeviceError::NoSuchBuffer));
    let whole = Mapping::whole(bo, 4096, VA, Access::ReadWrite);
    assert_eq!(dev.vm_bind(WINDOWD, ctx, whole), Err(DeviceError::NoSuchBuffer));
}

#[test]
fn two_contexts_reach_their_own_memory_through_the_same_address() {
    let mut dev = CpuMockDevice::phase2();
    let commands = fill();
    let mut own = Vec::new();
    for client in [WINDOWD, APP] {
        let ctx = dev.create_context(client, Priority::Normal).unwrap();
        let bo = dev.create_bo(client, 2 * 4096, CacheMode::CpuCached).unwrap();
        // The picture is the buffer's second page: a range, not the whole buffer.
        let m = Mapping { bo, offset: 4096, len: 4096, va: VA, access: Access::ReadWrite };
        dev.vm_bind(client, ctx, m).unwrap();
        own.push((client, ctx, bo));
    }
    let (windowd, app) = (own[0], own[1]);
    dev.submit(windowd.0, windowd.1, QueueKind::Render, &batch(VA, &commands, 1)).unwrap();
    let painted = |dev: &CpuMockDevice, (client, _, bo): (ClientId, ContextId, BoId)| {
        dev.bo_bytes(client, bo).unwrap()[4096 + (8 + 1) * 4] == 1
    };
    assert!(painted(&dev, windowd), "the range's page, at the binding's offset");
    assert!(!painted(&dev, app), "the same address in another context is other memory");
    assert_eq!(dev.bo_bytes(windowd.0, windowd.2).unwrap()[(8 + 1) * 4], 0);
    // A context goes with its bindings; the buffers stay their client's.
    dev.destroy_context(windowd.0, windowd.1).unwrap();
    assert!(painted(&dev, windowd));
    assert_eq!(dev.destroy_bo(windowd.0, windowd.2), Ok(()));
    assert_eq!(dev.destroy_bo(app.0, app.2), Err(DeviceError::BufferBound));
}

#[test]
fn test_reject_submit_references_unbound_va() {
    let (mut dev, ctx, _) = bound_device();
    let commands = fill();
    let elsewhere = GpuVa(0x20_0000);
    assert_eq!(
        dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(elsewhere, &commands, 1)),
        Err(DeviceError::Vm(VmError::Unbound))
    );
    // A target that starts inside the binding but runs past its end is unbound too.
    let late = GpuVa(VA.0 + 4096 - 16);
    assert_eq!(
        dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(late, &commands, 1)),
        Err(DeviceError::Vm(VmError::Unbound))
    );
    assert!(!dev.reached(TimelinePoint(1)), "nothing refused signals");
}

#[test]
fn test_reject_compute_queue_until_built() {
    let (mut dev, ctx, _) = bound_device();
    let commands = fill();
    assert_eq!(
        dev.submit(WINDOWD, ctx, QueueKind::Compute, &batch(VA, &commands, 1)),
        Err(DeviceError::QueueUnsupported(QueueKind::Compute))
    );
    assert!(!dev.capabilities().queue_kinds.executes(QueueKind::Compute));
}

#[test]
fn test_reject_oversized_batch() {
    let (mut dev, ctx, _) = bound_device();
    let one = fill();
    let commands: Vec<Command> = core::iter::repeat(one[0].clone()).take(257).collect();
    assert_eq!(
        dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(VA, &commands, 1)),
        Err(DeviceError::Batch(BatchError::TooLarge { commands: 257, max: 256 }))
    );
}

#[test]
fn test_reject_destroyed_context() {
    let (mut dev, ctx, bo) = bound_device();
    let commands = fill();
    assert_eq!(dev.destroy_context(APP, ctx), Err(DeviceError::NotOwner));
    dev.destroy_context(WINDOWD, ctx).unwrap();
    assert_eq!(
        dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(VA, &commands, 1)),
        Err(DeviceError::NoSuchContext)
    );
    assert_eq!(
        dev.vm_bind(WINDOWD, ctx, Mapping::whole(bo, 4096, VA, Access::ReadWrite)),
        Err(DeviceError::NoSuchContext)
    );
    assert_eq!(dev.destroy_bo(WINDOWD, bo), Ok(()), "the context's end unbound it");
    assert_eq!(dev.reset_context(WINDOWD, ctx), Err(DeviceError::NoSuchContext));
    let again = dev.create_context(WINDOWD, Priority::High).unwrap();
    assert_eq!(dev.context_state(again), Ok((Priority::High, 0)));
    dev.reset_context(WINDOWD, again).unwrap();
    assert_eq!(dev.context_state(again), Ok((Priority::High, 1)));
}

#[test]
fn test_reject_submit_on_a_lost_context() {
    let (mut dev, ctx, bo) = bound_device();
    let other = dev.create_context(APP, Priority::Normal).unwrap();
    let theirs = dev.create_bo(APP, 4096, CacheMode::CpuCached).unwrap();
    dev.vm_bind(APP, other, Mapping::whole(theirs, 4096, VA, Access::ReadWrite)).unwrap();
    let commands = fill();
    dev.lose_context(ctx).unwrap();
    let lost = DeviceError::ContextLost;
    assert_eq!(dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(VA, &commands, 1)), Err(lost));
    let elsewhere = Mapping::whole(bo, 4096, GpuVa(0x40_0000), Access::ReadWrite);
    assert_eq!(dev.vm_bind(WINDOWD, ctx, elsewhere), Err(lost));
    assert_eq!(dev.reset_context(WINDOWD, ctx), Err(lost));
    assert!(!dev.reached(TimelinePoint(1)), "nothing refused signals");
    // The other context runs on; the lost one is destroyed by its client, its buffer freed.
    assert_eq!(
        dev.submit(APP, other, QueueKind::Render, &batch(VA, &commands, 1)),
        Ok(TimelinePoint(1))
    );
    dev.destroy_context(WINDOWD, ctx).unwrap();
    assert_eq!(dev.destroy_bo(WINDOWD, bo), Ok(()));
    let again = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    assert_eq!(
        dev.vm_bind(WINDOWD, again, Mapping::whole(bo, 4096, VA, Access::ReadWrite)),
        Err(DeviceError::NoSuchBuffer)
    );
}

#[test]
fn test_reject_gpu_code_for_another_target() {
    let (mut dev, ctx, _) = bound_device();
    let code = [0u8; 16];
    let foreign = Shader { target: CodeTarget(1), entry: 0, bytes: &code };
    let own = Shader { target: dev.capabilities().gpu_code_target, entry: 0, bytes: &code };
    let at = GpuVa(VA.0 + 1024);
    let run = |dev: &mut CpuMockDevice, shader| {
        let b = Batch {
            target: target(VA),
            stream: Stream::Code { shader, at },
            waits: &[],
            signal: TimelinePoint(1),
        };
        dev.submit(WINDOWD, ctx, QueueKind::Render, &b)
    };
    assert_eq!(
        run(&mut dev, foreign),
        Err(DeviceError::CodeTargetMismatch {
            declared: CodeTarget(1),
            device: dev.capabilities().gpu_code_target
        })
    );
    assert_eq!(run(&mut dev, own), Err(DeviceError::CodeUnsupported), "declared, not run");
    let unbound = Batch {
        target: target(VA),
        stream: Stream::Code { shader: own, at: GpuVa(0x30_0000) },
        waits: &[],
        signal: TimelinePoint(1),
    };
    assert_eq!(
        dev.submit(WINDOWD, ctx, QueueKind::Render, &unbound),
        Err(DeviceError::Vm(VmError::Unbound))
    );
}

#[test]
fn a_wait_the_mock_cannot_defer_is_refused_not_ignored() {
    let (mut dev, ctx, _) = bound_device();
    let commands = fill();
    let waits = [TimelinePoint(7)];
    let b = Batch {
        target: target(VA),
        stream: Stream::Commands(&commands),
        waits: &waits,
        signal: TimelinePoint(8),
    };
    assert_eq!(
        dev.submit(WINDOWD, ctx, QueueKind::Render, &b),
        Err(DeviceError::WouldBlock(TimelinePoint(7)))
    );
    dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(VA, &commands, 7)).unwrap();
    assert_eq!(dev.submit(WINDOWD, ctx, QueueKind::Render, &b), Ok(TimelinePoint(8)));
    let full = CpuMockDevice::new(Capabilities { max_contexts: 1, ..dev.capabilities() });
    let mut full = full;
    full.create_context(APP, Priority::Low).unwrap();
    assert_eq!(full.create_context(APP, Priority::Low), Err(DeviceError::Exhausted));
}

/// The compositor's library as windowd supplies it, built for the board's code target.
const LIBRARY: &[u8] = include_bytes!("../../../../../../resources/gpu/pipelines/compositor.nxgp");

/// A mock with the board's features: every entry of the library runs on it.
fn library_device() -> CpuMockDevice {
    let caps = CpuMockDevice::phase2().capabilities();
    CpuMockDevice::new(Capabilities { gpu_code_features: feature::FUSED_ITERATE_SAMPLE, ..caps })
}

#[test]
fn a_context_takes_its_library_as_the_board_places_it() {
    let mut dev = library_device();
    let ctx = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    // The board placed the same bytes alike (cycle 25: `gpu library ok (… entries=7 refused=0
    // code_bytes=2808 setup_bytes=852 …`).
    assert_eq!(
        dev.take_library(WINDOWD, ctx, LIBRARY),
        Ok(LibraryTaken {
            accepted: 7,
            refused: 0,
            first_refused: 0,
            code_bytes: 2808,
            setup_bytes: 852
        })
    );
}

#[test]
fn test_reject_second_library_for_a_context() {
    let mut dev = library_device();
    let ctx = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    dev.take_library(WINDOWD, ctx, LIBRARY).unwrap();
    assert_eq!(dev.take_library(WINDOWD, ctx, LIBRARY), Err(DeviceError::LibraryHeld));
    // A restarted client's fresh context takes it again; the old number is gone.
    dev.destroy_context(WINDOWD, ctx).unwrap();
    let fresh = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    assert_eq!(dev.take_library(WINDOWD, fresh, LIBRARY).map(|t| t.accepted), Ok(7));
    assert_eq!(dev.take_library(WINDOWD, ctx, LIBRARY), Err(DeviceError::NoSuchContext));
}

#[test]
fn test_reject_library_into_another_clients_context() {
    let mut dev = library_device();
    let ctx = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    assert_eq!(dev.take_library(APP, ctx, LIBRARY), Err(DeviceError::NotOwner));
    dev.lose_context(ctx).unwrap();
    assert_eq!(dev.take_library(WINDOWD, ctx, LIBRARY), Err(DeviceError::ContextLost));
}

#[test]
fn test_reject_library_for_another_target_truncated_or_beyond_the_features() {
    let foreign =
        Capabilities { gpu_code_target: CodeTarget(1), ..library_device().capabilities() };
    let mut dev = CpuMockDevice::new(foreign);
    let ctx = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    assert!(matches!(
        dev.take_library(WINDOWD, ctx, LIBRARY),
        Err(DeviceError::Library(LibraryError::WrongTarget { .. }))
    ));
    let mut dev = library_device();
    let ctx = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
    assert!(matches!(
        dev.take_library(WINDOWD, ctx, &LIBRARY[..LIBRARY.len() - 1]),
        Err(DeviceError::Library(_))
    ));
    // A device without the feature refuses the entries that ask for it and keeps the rest.
    let mut plain = CpuMockDevice::phase2();
    let ctx = plain.create_context(WINDOWD, Priority::Realtime).unwrap();
    let taken = plain.take_library(WINDOWD, ctx, LIBRARY).unwrap();
    assert!(taken.refused > 0 && taken.first_refused != 0, "{taken:?}");
    assert_eq!(taken.accepted + taken.refused, 7);
}
