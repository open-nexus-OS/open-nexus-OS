// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The scene bundle's model tests (TASK-0169B / TASK-0215, RFC-0101 §8c/§8d): buffers, runs and
//! jobs round-trip through the builder and the parser; the per-buffer flags travel and unknown
//! bits are refused; the parser's reject matrix (another core, a run outside its buffer, a
//! buffer outside the heaps, overlaps, a missing target, truncation, a foreign header); the
//! checker texture is the capture program's.

use nexus_gfx::backend::gpu::scene::*;
use nexus_gfx::backend::gpu::{heaps, mmu, CoreId};

const PAGE: u64 = mmu::PAGE;

fn bundle(target: u64, data_runs: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut b = Builder::new(CoreId::BOARD, Texture::CheckerV1);
    b.buffer(heaps::GENERAL_BASE, PAGE, Kind::Data, data_runs);
    b.buffer(target, 2 * PAGE, Kind::Target, &[]);
    b.buffer(heaps::USC_CODE_BASE, PAGE, Kind::Texture, &[]);
    b.job(JobKind::Transfer, 1, &[0xaa; 176]);
    b.finish()
}

#[test]
fn buffer_flags_round_trip_and_unknown_bits_are_refused() {
    let mut b = Builder::new(CoreId::BOARD, Texture::None);
    b.buffer_with_flags(heaps::GENERAL_BASE, PAGE, Kind::Data, buffer_flags::DEVICE_CACHED, &[]);
    b.buffer_with_flags(
        heaps::GENERAL_BASE + PAGE,
        PAGE,
        Kind::Data,
        buffer_flags::PM_FW_PROTECT,
        &[],
    );
    b.buffer(heaps::GENERAL_BASE + 2 * PAGE, PAGE, Kind::Target, &[]);
    let bytes = b.finish();
    let s = Scene::parse(&bytes, CoreId::BOARD).unwrap();
    let bufs: Vec<Buffer<'_>> = s.buffers().collect();
    assert!(bufs[0].device_cached() && !bufs[0].pm_fw_protect());
    assert!(!bufs[1].device_cached() && bufs[1].pm_fw_protect());
    assert_eq!(
        (bufs[2].kind, bufs[2].flags),
        (Kind::Target, 0),
        "no flags: out of the GPU cache, as before"
    );
    // An unknown bit above the kind byte is refused, not ignored.
    let mut bad = bytes.clone();
    bad[HEADER_BYTES + 16..HEADER_BYTES + 20].copy_from_slice(&(1u32 << 12).to_le_bytes());
    assert_eq!(Scene::parse(&bad, CoreId::BOARD).err(), Some(SceneError::Kind(1 << 12)));
}

#[test]
fn round_trips_buffers_runs_and_jobs() {
    let runs = vec![(16u32, vec![1u8, 2, 3]), (4000, vec![9u8; 96])];
    let bytes = bundle(heaps::GENERAL_BASE + PAGE, &runs);
    let s = Scene::parse(&bytes, CoreId::BOARD).unwrap();
    assert_eq!((s.buffer_count, s.job_count, s.texture), (3, 1, Texture::CheckerV1));
    let bufs: Vec<Buffer<'_>> = s.buffers().collect();
    assert_eq!(bufs.len(), 3);
    let got: Vec<(u32, Vec<u8>)> = bufs[0].runs().map(|r| (r.off, r.bytes.to_vec())).collect();
    assert_eq!(got, runs);
    assert_eq!(s.target().map(|t| (t.va, t.pages())), Some((heaps::GENERAL_BASE + PAGE, 2)));
    let job = s.jobs().next().unwrap();
    assert_eq!((job.kind, job.single_core(), job.stream.len()), (JobKind::Transfer, true, 176));
    assert_eq!(s.data_bytes(), 99);
}

#[test]
fn test_reject_scene_for_another_core() {
    let bytes = bundle(heaps::GENERAL_BASE + PAGE, &[]);
    let other = CoreId::new(36, 53, 104, 796);
    assert_eq!(
        Scene::parse(&bytes, other).err(),
        Some(SceneError::Core { scene: CoreId::BOARD.0, device: other.0 })
    );
}

#[test]
fn test_reject_scene_run_outside_buffer() {
    let bytes = bundle(heaps::GENERAL_BASE + PAGE, &[(4090, vec![1u8; 8])]);
    assert_eq!(
        Scene::parse(&bytes, CoreId::BOARD).err(),
        Some(SceneError::RunOutsideBuffer { va: heaps::GENERAL_BASE, off: 4090, len: 8 })
    );
}

#[test]
fn test_reject_scene_buffer_outside_heaps() {
    let bytes = bundle(heaps::FIRMWARE_BASE, &[]);
    assert_eq!(
        Scene::parse(&bytes, CoreId::BOARD).err(),
        Some(SceneError::BufferOutsideHeaps { va: heaps::FIRMWARE_BASE, len: 2 * PAGE })
    );
    let unaligned = bundle(heaps::GENERAL_BASE + PAGE + 16, &[]);
    assert_eq!(
        Scene::parse(&unaligned, CoreId::BOARD).err(),
        Some(SceneError::Unaligned { va: heaps::GENERAL_BASE + PAGE + 16 })
    );
}

#[test]
fn test_reject_overlapping_buffers_and_missing_target() {
    let overlap = bundle(heaps::GENERAL_BASE, &[]);
    assert_eq!(
        Scene::parse(&overlap, CoreId::BOARD).err(),
        Some(SceneError::Overlap { va: heaps::GENERAL_BASE })
    );
    let mut b = Builder::new(CoreId::BOARD, Texture::None);
    b.buffer(heaps::GENERAL_BASE, PAGE, Kind::Data, &[]);
    assert_eq!(Scene::parse(&b.finish(), CoreId::BOARD).err(), Some(SceneError::NoTarget));
}

#[test]
fn test_reject_truncated_and_foreign_headers() {
    let bytes = bundle(heaps::GENERAL_BASE + PAGE, &[(0, vec![7u8; 40])]);
    assert_eq!(
        Scene::parse(&bytes[..bytes.len() - 100], CoreId::BOARD).err(),
        Some(SceneError::Truncated)
    );
    assert_eq!(Scene::parse(&bytes[..8], CoreId::BOARD).err(), Some(SceneError::Short));
    let mut magic = bytes.clone();
    magic[0] = b'X';
    assert_eq!(Scene::parse(&magic, CoreId::BOARD).err(), Some(SceneError::Magic));
    let mut version = bytes.clone();
    version[4] = 9;
    assert_eq!(Scene::parse(&version, CoreId::BOARD).err(), Some(SceneError::Version(9)));
}

#[test]
fn checker_v1_is_the_capture_programs_texture() {
    let mut tex = vec![0u8; 256 * 1024];
    checker_v1(256, 256, 1024, &mut tex);
    assert_eq!(&tex[0..4], &[255, 255, 255, 0], "frame, alpha 0 at the origin");
    let at = |x: usize, y: usize| &tex[y * 1024 + x * 4..y * 1024 + x * 4 + 4];
    assert_eq!(&at(20, 20)[..3], &[40, 90, 200]);
    assert_eq!(&at(52, 20)[..3], &[230, 120, 30]);
    assert_eq!(at(255, 255)[3], 255, "alpha 1 at the far corner");
    // The digest of the captured texture bytes (2026-10-13, `texture.sha256`'s twin).
    assert_eq!(fnv1a64(&tex), CHECKER_V1_FNV1A64);
}
