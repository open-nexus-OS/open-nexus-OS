// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The scene bundle (TASK-0169B P1, RFC-0101 §8c): GPU code as client data, captured on the
//! host from the open userspace driver for one core and replayed by the device. A bundle
//! (`NXSC`, version 1) names the core it was captured for and carries the **buffers** — each a
//! GPU address, a length, a kind and sparse non-zero runs — and the **jobs** with their
//! streams. A `Data` buffer is the driver's bytes (programs, state words, static areas); the
//! `Target` is bound to a plane of the ring and never carried; a `Texture` is generated on the
//! device by a named formula whose digest is a golden. The parser treats the file as untrusted:
//! the core id must be the device's, every buffer page-aligned inside one heap, every run inside
//! its buffer, exactly one target, no two buffers overlapping. [`pack`] is the packer's half
//! (the builder, the sparse runs); `examples/scene_pack.rs` runs it over a capture directory.

use alloc::vec::Vec;

use super::fwif::{FreelistArgs, HwrtArgs};
use super::{heaps, mmu, CoreId};

mod pack;
mod render;

pub use pack::{runs_of, Builder};
use render::read_render;
pub use render::RenderSetup;

pub const MAGIC: [u8; 4] = *b"NXSC";
/// Version 1: buffers and transfer jobs. Version 2 adds the render trailer (§8d).
pub const VERSION: u32 = 1;
pub const VERSION_RENDER: u32 = 2;
pub const HEADER_BYTES: usize = 32;
/// Buffers a bundle may carry — the first quad binds 28.
pub const MAX_BUFFERS: u32 = 256;
pub const MAX_JOBS: u32 = 64;
/// Bytes of one stream, bounded before the decoder sees it.
pub const MAX_STREAM_BYTES: u32 = 4096;

/// What a buffer is to the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The driver's bytes, carried as runs.
    Data = 0,
    /// The picture: a plane of the ring, bound by the device, never carried.
    Target = 1,
    /// Generated on the device by the bundle's texture formula.
    Texture = 2,
}

/// The queue a job runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Transfer = 0,
    /// A render pass's geometry job (RFC-0101 §8d).
    Geometry = 1,
    /// The partial-render fragment job that stands ready behind the geometry.
    FragmentPartial = 2,
    /// The fragment job proper.
    Fragment = 3,
}

/// The texture formula a bundle names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Texture {
    None = 0,
    /// [`checker_v1`]: a 32-px checker of two colours under a diagonal alpha ramp, 4-px frame.
    CheckerV1 = 1,
}

/// Why a bundle was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneError {
    Short,
    Magic,
    Version(u32),
    /// Captured for another core.
    Core {
        scene: u64,
        device: u64,
    },
    Count,
    Kind(u32),
    JobKind(u32),
    Texture(u32),
    Truncated,
    Unaligned {
        va: u64,
    },
    BufferOutsideHeaps {
        va: u64,
        len: u64,
    },
    RunOutsideBuffer {
        va: u64,
        off: u32,
        len: u32,
    },
    Overlap {
        va: u64,
    },
    NoTarget,
    ManyTargets,
    StreamTooLong(u32),
    /// A render trailer with other than the local and the global free list.
    Freelists(u32),
    /// A static render-context state that is not the 23 registers' stream.
    StaticState(u32),
    /// Render jobs without the render trailer, or a trailer without render jobs.
    RenderSetup,
}

pub(super) fn get32(b: &[u8], off: usize) -> Result<u32, SceneError> {
    let s = b.get(off..off + 4).ok_or(SceneError::Truncated)?;
    let mut w = [0u8; 4];
    w.copy_from_slice(s);
    Ok(u32::from_le_bytes(w))
}

pub(super) fn get64(b: &[u8], off: usize) -> Result<u64, SceneError> {
    let s = b.get(off..off + 8).ok_or(SceneError::Truncated)?;
    let mut w = [0u8; 8];
    w.copy_from_slice(s);
    Ok(u64::from_le_bytes(w))
}

pub(super) const fn align4(n: usize) -> usize {
    (n + 3) & !3
}

pub(super) const fn align8(n: usize) -> usize {
    (n + 7) & !7
}

/// The heap a GPU address range lies in, whole.
fn in_one_heap(va: u64, len: u64) -> bool {
    let ranges = [
        (heaps::GENERAL_BASE, heaps::GENERAL_SIZE),
        (heaps::PDS_CODE_DATA_BASE, heaps::PDS_CODE_DATA_SIZE),
        (heaps::USC_CODE_BASE, heaps::USC_CODE_SIZE),
        (heaps::TRANSFER_FRAG_BASE, heaps::TRANSFER_FRAG_SIZE),
        (heaps::RGNHDR_BASE, heaps::RGNHDR_SIZE),
        (heaps::VIS_TEST_BASE, heaps::VIS_TEST_SIZE),
    ];
    let Some(end) = va.checked_add(len) else {
        return false;
    };
    ranges.iter().any(|&(base, size)| va >= base && end <= base + size)
}

/// One non-zero run of a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run<'a> {
    pub off: u32,
    pub bytes: &'a [u8],
}

/// One buffer of the bundle.
#[derive(Clone, Copy, Debug)]
pub struct Buffer<'a> {
    pub va: u64,
    pub len: u64,
    pub kind: Kind,
    pub run_count: u32,
    runs: &'a [u8],
}

impl<'a> Buffer<'a> {
    /// The runs, in file order.
    pub fn runs(&self) -> RunIter<'a> {
        RunIter { bytes: self.runs, off: 0, left: self.run_count }
    }

    /// Pages of the buffer.
    pub const fn pages(&self) -> u64 {
        self.len / mmu::PAGE
    }
}

pub struct RunIter<'a> {
    bytes: &'a [u8],
    off: usize,
    left: u32,
}

impl<'a> Iterator for RunIter<'a> {
    type Item = Run<'a>;
    fn next(&mut self) -> Option<Run<'a>> {
        if self.left == 0 {
            return None;
        }
        let (run, next) = read_run(self.bytes, self.off).ok()?;
        self.off = next;
        self.left -= 1;
        Some(run)
    }
}

fn read_run(b: &[u8], off: usize) -> Result<(Run<'_>, usize), SceneError> {
    let run_off = get32(b, off)?;
    let len = get32(b, off + 4)? as usize;
    let start = off + 8;
    let bytes = b.get(start..start + len).ok_or(SceneError::Truncated)?;
    Ok((Run { off: run_off, bytes }, start + align4(len)))
}

/// Reads one buffer record at `off`; validates its runs. Returns it and the next offset.
fn read_buffer(b: &[u8], off: usize) -> Result<(Buffer<'_>, usize), SceneError> {
    let va = get64(b, off)?;
    let len = get64(b, off + 8)?;
    let kind = match get32(b, off + 16)? {
        0 => Kind::Data,
        1 => Kind::Target,
        2 => Kind::Texture,
        k => return Err(SceneError::Kind(k)),
    };
    let run_count = get32(b, off + 20)?;
    if va % mmu::PAGE != 0 || len % mmu::PAGE != 0 || len == 0 {
        return Err(SceneError::Unaligned { va });
    }
    if !in_one_heap(va, len) {
        return Err(SceneError::BufferOutsideHeaps { va, len });
    }
    let runs_start = off + 24;
    let mut cursor = runs_start;
    for _ in 0..run_count {
        let (run, next) = read_run(b, cursor)?;
        let run_len = run.bytes.len() as u32;
        let end = u64::from(run.off) + u64::from(run_len);
        if end > len {
            return Err(SceneError::RunOutsideBuffer { va, off: run.off, len: run_len });
        }
        cursor = next;
    }
    let runs = &b[runs_start..cursor];
    Ok((Buffer { va, len, kind, run_count, runs }, cursor))
}

/// One job of the bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job<'a> {
    pub kind: JobKind,
    /// The open driver's submit flags (bit 0: single core).
    pub flags: u32,
    pub stream: &'a [u8],
}

impl Job<'_> {
    pub const fn single_core(&self) -> bool {
        self.flags & 1 != 0
    }
}

fn read_job(b: &[u8], off: usize) -> Result<(Job<'_>, usize), SceneError> {
    let kind = match get32(b, off)? {
        0 => JobKind::Transfer,
        1 => JobKind::Geometry,
        2 => JobKind::FragmentPartial,
        3 => JobKind::Fragment,
        k => return Err(SceneError::JobKind(k)),
    };
    let flags = get32(b, off + 4)?;
    let len = get32(b, off + 8)?;
    if len > MAX_STREAM_BYTES {
        return Err(SceneError::StreamTooLong(len));
    }
    let start = off + 16;
    let stream = b.get(start..start + len as usize).ok_or(SceneError::Truncated)?;
    Ok((Job { kind, flags, stream }, start + align8(len as usize)))
}

/// A parsed bundle: validated once, iterated without allocation.
#[derive(Clone, Copy, Debug)]
pub struct Scene<'a> {
    pub core: CoreId,
    pub texture: Texture,
    pub buffer_count: u32,
    pub job_count: u32,
    buffers: &'a [u8],
    jobs: &'a [u8],
    render: Option<RenderSetup<'a>>,
}

impl<'a> Scene<'a> {
    /// Parses and validates `bytes` for `device`'s core.
    pub fn parse(bytes: &'a [u8], device: CoreId) -> Result<Self, SceneError> {
        if bytes.len() < HEADER_BYTES {
            return Err(SceneError::Short);
        }
        if bytes[0..4] != MAGIC {
            return Err(SceneError::Magic);
        }
        let version = get32(bytes, 4)?;
        if version != VERSION && version != VERSION_RENDER {
            return Err(SceneError::Version(version));
        }
        let core = get64(bytes, 8)?;
        if core != device.0 {
            return Err(SceneError::Core { scene: core, device: device.0 });
        }
        let buffer_count = get32(bytes, 16)?;
        let job_count = get32(bytes, 20)?;
        let texture = match get32(bytes, 24)? {
            0 => Texture::None,
            1 => Texture::CheckerV1,
            t => return Err(SceneError::Texture(t)),
        };
        if buffer_count > MAX_BUFFERS || job_count > MAX_JOBS {
            return Err(SceneError::Count);
        }
        let mut off = HEADER_BYTES;
        let buffers_start = off;
        let mut targets = 0u32;
        let mut seen: Vec<(u64, u64)> = Vec::new();
        for _ in 0..buffer_count {
            let (buf, next) = read_buffer(bytes, off)?;
            if seen.iter().any(|&(va, len)| buf.va < va + len && va < buf.va + buf.len) {
                return Err(SceneError::Overlap { va: buf.va });
            }
            seen.push((buf.va, buf.len));
            if buf.kind == Kind::Target {
                targets += 1;
            }
            off = next;
        }
        match targets {
            0 => return Err(SceneError::NoTarget),
            1 => {}
            _ => return Err(SceneError::ManyTargets),
        }
        let buffers = &bytes[buffers_start..off];
        let jobs_start = off;
        for _ in 0..job_count {
            let (_, next) = read_job(bytes, off)?;
            off = next;
        }
        let jobs = &bytes[jobs_start..off];
        let scene = Self {
            core: CoreId(core),
            texture,
            buffer_count,
            job_count,
            buffers,
            jobs,
            render: None,
        };
        let has_render_jobs = scene.jobs().any(|j| j.kind != JobKind::Transfer);
        let render = if version == VERSION_RENDER {
            let (setup, next) = read_render(bytes, off)?;
            if !has_render_jobs || next > bytes.len() {
                return Err(SceneError::RenderSetup);
            }
            Some(setup)
        } else {
            if has_render_jobs {
                return Err(SceneError::RenderSetup);
            }
            None
        };
        Ok(Self { render, ..scene })
    }

    /// The render pass's kernel-side objects (version 2 bundles).
    pub fn render(&self) -> Option<RenderSetup<'a>> {
        self.render
    }

    pub fn buffers(&self) -> BufferIter<'a> {
        BufferIter { bytes: self.buffers, off: 0, left: self.buffer_count }
    }

    pub fn jobs(&self) -> JobIter<'a> {
        JobIter { bytes: self.jobs, off: 0, left: self.job_count }
    }

    /// The one target.
    pub fn target(&self) -> Option<Buffer<'a>> {
        self.buffers().find(|b| b.kind == Kind::Target)
    }

    /// Bytes of data the device writes before the first job (runs only).
    pub fn data_bytes(&self) -> u64 {
        self.buffers().flat_map(|b| b.runs()).map(|r| r.bytes.len() as u64).sum()
    }
}

pub struct BufferIter<'a> {
    bytes: &'a [u8],
    off: usize,
    left: u32,
}

impl<'a> Iterator for BufferIter<'a> {
    type Item = Buffer<'a>;
    fn next(&mut self) -> Option<Buffer<'a>> {
        if self.left == 0 {
            return None;
        }
        let (buf, next) = read_buffer(self.bytes, self.off).ok()?;
        self.off = next;
        self.left -= 1;
        Some(buf)
    }
}

pub struct JobIter<'a> {
    bytes: &'a [u8],
    off: usize,
    left: u32,
}

impl<'a> Iterator for JobIter<'a> {
    type Item = Job<'a>;
    fn next(&mut self) -> Option<Job<'a>> {
        if self.left == 0 {
            return None;
        }
        let (job, next) = read_job(self.bytes, self.off).ok()?;
        self.off = next;
        self.left -= 1;
        Some(job)
    }
}

/// The texture of the first quad (`Texture::CheckerV1`), RGBA8 rows of `pitch` bytes: a 32-px
/// checker of (230,120,30) and (40,90,200) under a diagonal alpha ramp, a 4-px white frame —
/// the capture program's formula, byte for byte (`docs/board/measurements/2026-10-13-gpu-
/// first-quad/capture/first_quad.c`).
pub fn checker_v1(width: u32, height: u32, pitch: u32, out: &mut [u8]) {
    for y in 0..height {
        for x in 0..width {
            let frame = x < 4 || y < 4 || x >= width - 4 || y >= height - 4;
            let check = ((x >> 5) ^ (y >> 5)) & 1 == 1;
            let (r, g, b) = if frame {
                (255, 255, 255)
            } else if check {
                (230, 120, 30)
            } else {
                (40, 90, 200)
            };
            let a = ((x + y) * 255 / (width + height - 2)) as u8;
            let at = (y * pitch + x * 4) as usize;
            if let Some(px) = out.get_mut(at..at + 4) {
                px.copy_from_slice(&[r, g, b, a]);
            }
        }
    }
}

/// FNV-1a over 64 bits: the digest a golden pins a generated texture with.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn round_trips_buffers_runs_and_jobs() {
        let runs = alloc::vec![(16u32, alloc::vec![1u8, 2, 3]), (4000, alloc::vec![9u8; 96])];
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
        let bytes = bundle(heaps::GENERAL_BASE + PAGE, &[(4090, alloc::vec![1u8; 8])]);
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
        let bytes = bundle(heaps::GENERAL_BASE + PAGE, &[(0, alloc::vec![7u8; 40])]);
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
        let mut tex = alloc::vec![0u8; 256 * 1024];
        checker_v1(256, 256, 1024, &mut tex);
        assert_eq!(&tex[0..4], &[255, 255, 255, 0], "frame, alpha 0 at the origin");
        let at = |x: usize, y: usize| &tex[y * 1024 + x * 4..y * 1024 + x * 4 + 4];
        assert_eq!(&at(20, 20)[..3], &[40, 90, 200]);
        assert_eq!(&at(52, 20)[..3], &[230, 120, 30]);
        assert_eq!(at(255, 255)[3], 255, "alpha 1 at the far corner");
        // The digest of the captured texture bytes (2026-10-13, `texture.sha256`'s twin).
        assert_eq!(fnv1a64(&tex), CHECKER_V1_FNV1A64);
    }
}

/// FNV-1a 64 of the 256×256 checker the capture program wrote (2026-10-13).
pub const CHECKER_V1_FNV1A64: u64 = 0x940b_4489_8e74_4af5;
