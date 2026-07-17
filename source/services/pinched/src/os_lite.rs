// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite backend for pinched — the compute-broker server loop.
//! Jobs arrive as `OP_COMPUTE` frames with the data VMO attached via CAP_MOVE
//! (zero IPC-frame copies; the RFC-0072 splice pattern). The broker computes
//! in place on the shared nexus-workpool (fence-coordinated, deterministic
//! chunking) and completes by writing the VMO header LAST (release fence).
//! Bounded everything: oversized jobs are rejected via the header; a workpool
//! failure falls back to the inline path (fail-open on LOCAL compute, never
//! on waiting) and reports `workers = 0` honestly.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU markers `SELFTEST: pinched determinism ok` /
//!   `pinched bounded ok`; broker transform host-tested in broker.rs.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::time::Duration;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{KernelServer, Server as _, Wait};

use alloc::sync::Arc;

use nexus_sync::SpinLock;

use crate::broker::mix_u32;
use crate::protocol::*;
use crate::{MAX_JOB_ELEMS, MAX_SVG_BYTES, MAX_SVG_JOB_DIM, PINCHED_WORKERS};

/// Result type for pinched operations.
pub type PinchedResult<T> = Result<T, PinchedError>;

/// Errors from the pinched service.
#[derive(Debug)]
pub enum PinchedError {
    /// IPC error.
    Ipc(&'static str),
}

impl core::fmt::Display for PinchedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Ipc(msg) => write!(f, "ipc: {}", msg),
        }
    }
}

/// Notifies init once the service reports readiness.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Static job buffer: the workpool workers and the server loop share it via
/// atomics (disjoint chunks per worker — no locks, no per-request allocation;
/// the service bump allocator never frees).
static JOB_BUF: [AtomicU32; MAX_JOB_ELEMS] = [const { AtomicU32::new(0) }; MAX_JOB_ELEMS];

/// Whether the workpool came up; false = inline fallback (workers = 0).
static POOL_READY: AtomicBool = AtomicBool::new(false);

/// One-shot receive crumb (proves the first job frame actually arrived).
static FIRST_JOB_SEEN: AtomicBool = AtomicBool::new(false);

/// The in-flight SVG job the workers rasterize from. The server publishes the
/// Arc BEFORE signalling the job fence and clears it after the done fence;
/// workers take the lock only long enough to clone the Arc (read-only share,
/// no lock held across the raster loop).
static SVG_JOB: SpinLock<Option<Arc<SvgJob>>> = SpinLock::new(None);

struct SvgJob {
    plan: nexus_svg::RasterPlan,
    w: usize,
}

/// Worker progress steps for the SVG job (diagnosis: where does it die?).
/// 1 = entered, 2 = plan cloned, 3 = scratch built, 4 = first row done,
/// 5 = all rows done.
static SVG_STEP: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];

/// The `JOB_MAP_MIX_U32` job on the shared buffer (workpool JobFn shape).
extern "C" fn job_map_mix(start: usize, end: usize, _ctx: *mut u8) {
    for slot in JOB_BUF.iter().take(end).skip(start) {
        slot.store(mix_u32(slot.load(Ordering::Relaxed)), Ordering::Relaxed);
    }
}

/// The `JOB_SVG_RASTER` job: rows `[start, end)` of the published plan.
/// Each row rasterizes into a stack buffer and lands as u32le pixels in the
/// shared atomic buffer (disjoint per-row ranges — no worker overlap). The
/// per-worker scratch is ONE bounded allocation per job (batch path; the
/// bump allocator never frees, so nothing here allocates per row).
extern "C" fn job_svg_rows(start: usize, end: usize, _ctx: *mut u8) {
    let step = &SVG_STEP[start % 4];
    step.store(1, Ordering::Release);
    let job = SVG_JOB.lock().clone();
    let Some(job) = job else {
        return;
    };
    step.store(2, Ordering::Release);
    let mut scratch = job.plan.scratch();
    step.store(3, Ordering::Release);
    let mut row = [0u8; MAX_SVG_JOB_DIM * 4];
    for y in start..end {
        let buf = &mut row[..job.w * 4];
        buf.fill(0);
        if job.plan.rasterize_rows(y as u32, y as u32 + 1, &mut scratch, buf).is_err() {
            // Bounded silent: the parent's digest check reports the mismatch.
            return;
        }
        let base = y * job.w;
        for (x, px) in buf.chunks_exact(4).enumerate() {
            let value = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
            JOB_BUF[base + x].store(value, Ordering::Relaxed);
        }
        step.store(4, Ordering::Release);
    }
    step.store(5, Ordering::Release);
}

/// Main service loop for pinched.
pub fn service_main_loop(notifier: ReadyNotifier) -> PinchedResult<()> {
    notifier.notify();
    emit_line("pinched: ready");

    let server = route_pinched_blocking().ok_or(PinchedError::Ipc("route failed"))?;

    // Bring up the compute backend once. Failure is NOT fatal: jobs then run
    // inline (deterministically identical result, workers reported as 0).
    match nexus_workpool::init(PINCHED_WORKERS) {
        Ok(()) => {
            POOL_READY.store(true, Ordering::Release);
            emit_line("pinched: workpool ready");
        }
        Err(_) => emit_line("pinched: workpool unavailable (inline fallback)"),
    }

    nexus_abi::service_verdict_flush("pinched");

    // Reused VMO I/O staging buffer (allocated once — never per request).
    let mut io_buf: Vec<u8> = Vec::with_capacity(MAX_JOB_ELEMS * 4);

    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, _sender_service_id, reply_cap)) => {
                let frame = frame.as_slice();
                if frame.len() >= MIN_FRAME_LEN
                    && frame[0] == MAGIC0
                    && frame[1] == MAGIC1
                    && frame[2] == VERSION
                    && frame[3] == OP_COMPUTE
                {
                    // The moved cap IS the job VMO (CAP_MOVE, not a reply
                    // endpoint); completion goes through its header.
                    let vmo_slot = reply_cap.map(|cap| {
                        let slot = cap.slot();
                        core::mem::forget(cap);
                        slot
                    });
                    if !FIRST_JOB_SEEN.swap(true, Ordering::Relaxed) {
                        emit_line(if vmo_slot.is_some() {
                            "pinched: first job recv (vmo)"
                        } else {
                            "pinched: first job recv (NO vmo)"
                        });
                    }
                    handle_compute(frame, vmo_slot, &mut io_buf);
                    continue;
                }
                // Non-compute traffic: plain frame reply on whichever path
                // the caller used (rngd's dual-path routing rule).
                let op = frame.get(3).copied().unwrap_or(0);
                let rsp =
                    [MAGIC0, MAGIC1, VERSION, op | OP_RESPONSE, STATUS_MALFORMED as u8];
                if let Some(reply) = reply_cap {
                    let _ = reply.reply_and_close(&rsp);
                } else {
                    let _ = server.send(&rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => {
                emit_line("pinched: recv disconnected");
                return Err(PinchedError::Ipc("disconnected"));
            }
            Err(_) => {
                emit_line("pinched: recv error");
                return Err(PinchedError::Ipc("recv"));
            }
        }
    }
}

/// Serves one `OP_COMPUTE`: validate → stage in → compute → stage out →
/// header LAST → close the moved cap. Every early exit still writes a header
/// (the client must never poll forever) and closes the cap.
fn handle_compute(frame: &[u8], vmo_slot: Option<u32>, io_buf: &mut Vec<u8>) {
    let Some(vmo) = vmo_slot else {
        // No cap arrived — nothing to complete on; the client's poll deadline
        // is the honest failure path.
        emit_line("pinched: FAIL compute (no vmo cap)");
        return;
    };
    if frame.len() < COMPUTE_REQ_LEN {
        return finish(vmo, STATUS_MALFORMED, 0, 0);
    }
    let kind = frame[4];
    let total = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]) as usize;
    match kind {
        JOB_MAP_MIX_U32 => {}
        JOB_SVG_RASTER => return handle_svg(frame, vmo, total, io_buf),
        _ => return finish(vmo, STATUS_BAD_KIND, 0, 0),
    }
    if frame.len() != COMPUTE_REQ_LEN {
        return finish(vmo, STATUS_MALFORMED, 0, 0);
    }
    if total == 0 || total > MAX_JOB_ELEMS {
        emit_line("pinched: job reject (oversize total)");
        return finish(vmo, STATUS_OVERSIZED, 0, 0);
    }
    let need = DATA_OFFSET + total * 4;
    if vmo_capacity(vmo).map_or(true, |len| len < need) {
        emit_line("pinched: job reject (vmo capacity)");
        return finish(vmo, STATUS_OVERSIZED, 0, 0);
    }

    // Stage in: VMO payload → shared atomic buffer.
    io_buf.clear();
    io_buf.resize(total * 4, 0);
    if nexus_abi::vmo_read(vmo, DATA_OFFSET, io_buf.as_mut_slice()).is_err() {
        return finish(vmo, STATUS_IO, 0, 0);
    }
    for (i, chunk) in io_buf.chunks_exact(4).enumerate() {
        let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        JOB_BUF[i].store(value, Ordering::Relaxed);
    }

    // Compute: workpool run Ok PROVES all workers finished their chunks
    // (the done fence requires done_count == workers); on any failure the
    // inline path produces the identical result with workers = 0.
    let workers: u32 = if POOL_READY.load(Ordering::Acquire)
        && nexus_workpool::run(total, job_map_mix, core::ptr::null_mut(), 5_000_000_000).is_ok()
    {
        PINCHED_WORKERS as u32
    } else {
        job_map_mix(0, total, core::ptr::null_mut());
        0
    };

    // Stage out: payload FIRST, header LAST (release fence for the poller).
    for (i, chunk) in io_buf.chunks_exact_mut(4).enumerate() {
        chunk.copy_from_slice(&JOB_BUF[i].load(Ordering::Relaxed).to_le_bytes());
    }
    if nexus_abi::vmo_write(vmo, DATA_OFFSET, io_buf.as_slice()).is_err() {
        return finish(vmo, STATUS_IO, 0, 0);
    }
    finish(vmo, STATUS_OK, total as u32, workers)
}

/// Serves one `JOB_SVG_RASTER`: parse + plan once, publish the plan, rasterize
/// row bands on the workpool, stage the pixels out. Same bounded/fail-open
/// contract as the mix job (inline fallback reports `workers = 0`).
fn handle_svg(frame: &[u8], vmo: u32, total: usize, io_buf: &mut Vec<u8>) {
    if frame.len() != COMPUTE_SVG_REQ_LEN {
        return finish(vmo, STATUS_MALFORMED, 0, 0);
    }
    let w = u16::from_le_bytes([frame[9], frame[10]]) as usize;
    let h = u16::from_le_bytes([frame[11], frame[12]]) as usize;
    let svg_len = u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]) as usize;
    if w == 0 || h == 0 || w > MAX_SVG_JOB_DIM || h > MAX_SVG_JOB_DIM || w * h > MAX_JOB_ELEMS {
        emit_line("pinched: svg reject (dimensions)");
        return finish(vmo, STATUS_OVERSIZED, 0, 0);
    }
    if total != h {
        return finish(vmo, STATUS_MALFORMED, 0, 0);
    }
    if svg_len == 0 || svg_len > MAX_SVG_BYTES {
        emit_line("pinched: svg reject (source size)");
        return finish(vmo, STATUS_OVERSIZED, 0, 0);
    }
    let need = DATA_OFFSET + svg_len.max(w * h * 4);
    if vmo_capacity(vmo).map_or(true, |len| len < need) {
        emit_line("pinched: svg reject (vmo capacity)");
        return finish(vmo, STATUS_OVERSIZED, 0, 0);
    }

    // Stage in: SVG source bytes.
    io_buf.clear();
    io_buf.resize(svg_len, 0);
    if nexus_abi::vmo_read(vmo, DATA_OFFSET, io_buf.as_mut_slice()).is_err() {
        return finish(vmo, STATUS_IO, 0, 0);
    }
    // Parse + plan ONCE; workers share the immutable plan via Arc. These are
    // bounded per-job allocations (batch path), not per-row.
    let Ok(src) = core::str::from_utf8(io_buf.as_slice()) else {
        return finish(vmo, STATUS_BAD_INPUT, 0, 0);
    };
    let plan = match nexus_svg::parse_svg(src)
        .and_then(|doc| nexus_svg::plan_document_at(&doc, w as u32, h as u32))
    {
        Ok(plan) => plan,
        Err(_) => {
            emit_line("pinched: svg reject (parse/plan)");
            return finish(vmo, STATUS_BAD_INPUT, 0, 0);
        }
    };
    *SVG_JOB.lock() = Some(Arc::new(SvgJob { plan, w }));

    // Compute over rows; run() Ok proves every worker finished its band.
    // Generous deadline: an SVG raster is a batch job and icount TCG
    // soft-float is slow; a timeout POISONS the pool (workers unknown), so
    // the deadline must dominate the worst honest run, not the average.
    let workers: u32 = if POOL_READY.load(Ordering::Acquire) {
        match nexus_workpool::run(h, job_svg_rows, core::ptr::null_mut(), 6_000_000_000) {
            Ok(()) => PINCHED_WORKERS as u32,
            Err(_) => {
                let (alive, woke, done) = nexus_workpool::pool::selftest_debug();
                emit_line(match (alive, woke > 0, done) {
                    (a, _, _) if a < 2 => "pinched: svg run FAIL (worker died)",
                    (_, false, _) => "pinched: svg run FAIL (workers never woke)",
                    (_, true, 0) => "pinched: svg run FAIL (woke, done=0)",
                    (_, true, 1) => "pinched: svg run FAIL (woke, done=1)",
                    _ => "pinched: svg run FAIL (woke, done=2?)",
                });
                // Which step did each worker band reach? (chunk starts: 0 and h/2)
                emit_line(match (
                    SVG_STEP[0].load(Ordering::Acquire),
                    SVG_STEP[(h / 2) % 4].load(Ordering::Acquire),
                ) {
                    (0, b) if b > 0 => "pinched: svg step a=0 (band0 never entered)",
                    (a, 0) if a > 0 => "pinched: svg step b=0 (band1 never entered)",
                    (0, 0) => "pinched: svg step both=0",
                    (1, _) | (_, 1) => "pinched: svg step stuck=1 (plan clone)",
                    (2, _) | (_, 2) => "pinched: svg step stuck=2 (scratch alloc)",
                    (3, _) | (_, 3) => "pinched: svg step stuck=3 (first row)",
                    (4, _) | (_, 4) => "pinched: svg step stuck=4 (mid rows)",
                    _ => "pinched: svg step both=5 (completed?!)",
                });
                job_svg_rows(0, h, core::ptr::null_mut());
                0
            }
        }
    } else {
        job_svg_rows(0, h, core::ptr::null_mut());
        0
    };
    *SVG_JOB.lock() = None;
    emit_line(if workers > 0 {
        "pinched: svg raster done (par)"
    } else {
        "pinched: svg raster done (inline)"
    });

    // Stage out: pixels FIRST, header LAST.
    io_buf.clear();
    io_buf.resize(w * h * 4, 0);
    for (i, chunk) in io_buf.chunks_exact_mut(4).enumerate() {
        chunk.copy_from_slice(&JOB_BUF[i].load(Ordering::Relaxed).to_le_bytes());
    }
    if nexus_abi::vmo_write(vmo, DATA_OFFSET, io_buf.as_slice()).is_err() {
        return finish(vmo, STATUS_IO, 0, 0);
    }
    finish(vmo, STATUS_OK, (w * h) as u32, workers)
}

/// Writes the completion header (the release fence) and closes the moved cap.
fn finish(vmo: u32, status: u32, elems: u32, workers: u32) {
    let mut hdr = [0u8; HDR_LEN];
    hdr[0..4].copy_from_slice(&DONE_MAGIC.to_le_bytes());
    hdr[4..8].copy_from_slice(&status.to_le_bytes());
    hdr[8..12].copy_from_slice(&elems.to_le_bytes());
    hdr[12..16].copy_from_slice(&workers.to_le_bytes());
    if nexus_abi::vmo_write(vmo, 0, &hdr).is_err() {
        emit_line("pinched: FAIL finish (hdr write)");
    }
    let _ = nexus_abi::cap_close(vmo);
}

/// VMO capacity via cap_query (kind_tag 1 = VMO; the vfsd splice pattern).
fn vmo_capacity(slot: u32) -> Option<usize> {
    let mut query = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    if nexus_abi::cap_query(slot, &mut query).is_err() || query.kind_tag != 1 {
        return None;
    }
    Some(query.len as usize)
}

fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    match budget::route_with_nonce_budgeted(
        name,
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    }
}

fn route_pinched_blocking() -> Option<KernelServer> {
    let (send_slot, recv_slot) = route_blocking(b"pinched")?;
    KernelServer::new_with_slots(recv_slot, send_slot).ok()
}

fn emit_line(message: &str) {
    if nexus_abi::service_line(message.as_bytes()) {
        return;
    }
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}
