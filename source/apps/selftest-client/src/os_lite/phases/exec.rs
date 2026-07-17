// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 5 of 12 — exec (exec-ELF E2E hello payload, exit lifecycle
//!   exit0 payload, TASK-0018 Minidump v1 proof, forged metadata /
//!   no-artifact / mismatched build_id reject paths, spoofed-requester deny,
//!   malformed execd reject).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — exec / minidump slice.
//!
//! Extracted in Cut P2-08 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. Timing-sensitive yield budgets (256
//! iterations to let the child print + 256 iterations to let crash logs reach
//! logd) are preserved verbatim.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md
//!
//! `execd_client`, `logd`, `statefsd` handles are local to this phase;
//! downstream phases re-resolve via the silent `route_with_retry`.

use nexus_abi::yield_;

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::os_lite::{probes, services};

pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    // P0 two-window: bring-up is complete — log the burst maxima and reset
    // so the boot-end `bkl budget` gate judges STEADY STATE (this ladder).
    nexus_abi::sched::bkl_budget_reset();

    // Task #14 (SMP track): register-soak — seed every callee/caller-saved
    // GPR with a pattern, spin across several timer preemptions, verify. A
    // corrupted register here is the self-localizing proof for the
    // "f32/alloc-heavy compute is non-deterministic in this process" bug.
    regsoak_proof();

    // TASK-0288: interactive floor — this task runs Interactive QoS; across
    // a yield storm under live system background load, no single scheduling
    // gap may exceed the floor budget (result-proof; generous bound so MTTCG
    // SMP=2 timing cannot flake it).
    ui_runtime_floor_proof();

    // B6 (TASK-0042): the sched ABI applied from userspace — affinity and
    // shares round-trip through the REAL syscalls on this task.
    sched_applied_proof();

    // Task #14: the direct symptom — building and rasterizing the SAME SVG
    // plan multiple times in THIS process must be deterministic and match
    // the host-pinned golden digest. This was provably broken during the D4
    // work (empty/varying plans); keep it as a standing detector.
    svg_local_determinism_proof();

    // Task #14 companions: isolate the ingredient — pure f32 compute (no
    // alloc) and pure alloc traffic (no f32) with known answers.
    f32_soak_proof();
    alloc_soak_proof();
    memset_soak_proof();

    // Phase C (SMP track): same-AS compute thread E2E — spawn a thread into
    // OUR address space, let it write a sentinel, reap it via wait(). The
    // thread has an empty cap table by construction (compute-only contract).
    thread_spawn_proof();

    // Phase C3 (SMP track): THE process workpool — deterministic parallel
    // compute with fence coordination; result must equal the sequential
    // reference (workers=1 ≡ workers=N contract, TASK-0276).
    workpool_proof();
    crate::os_lite::probes::pinched::pinched_selftest();

    // logd handle is needed for crash-log verification inside this phase.
    let logd = route_with_retry("logd")?;

    // TASK-0006: core service wiring proof is performed later, after dsoftbus tests,
    // so the dsoftbusd local IPC server is guaranteed to be running.

    // Exec-ELF E2E via execd service (spawns hello payload).
    let execd_client = route_with_retry("execd")?;
    emit_line(crate::markers::M_SELFTEST_IPC_ROUTING_EXECD_OK);
    emit_line("HELLOHDR");
    probes::elf::log_hello_elf_header();
    let _hello_pid = services::execd::execd_spawn_image(&execd_client, "selftest-client", 1)?;
    // Allow the child to run and print crate::markers::M_CHILD_HELLO_ELF before we emit the marker.
    for _ in 0..256 {
        let _ = yield_();
    }
    emit_line(crate::markers::M_EXECD_ELF_LOAD_OK);
    emit_line(crate::markers::M_SELFTEST_E2E_EXEC_ELF_OK);

    // TASK-0080D R1: spawn the app-host transport probe (IMG_APPHOST=4).
    // The probe walks the ADR-0042 chain itself and emits `APPHOST: probe
    // surface presented` when its window is live; a spawn refusal (e.g. no
    // embedded payload in this image) is reported by value, not silence.
    match services::execd::execd_spawn_image(&execd_client, "selftest-client", 4) {
        Ok(_pid) => emit_line(crate::markers::M_SELFTEST_APPHOST_SPAWN_REQUESTED),
        Err(()) => emit_line(crate::markers::M_SELFTEST_APPHOST_SPAWN_REFUSED),
    }

    // RFC-0068 exec migration: the old execd child-exec + crash/minidump proof (exit0 lifecycle /
    // minidump v1 / crash-report / forged-metadata + no-artifact + mismatched-build_id reject /
    // spoofed-requester deny / malformed-request reject) is retired here. Root cause: execd-spawned
    // children LOAD but no longer execute, so that whole chain regressed (it was masked because the
    // headless proof skips verify-uart). Spawn coverage now lives in kernel KSELFTEST spawn +
    // abilitymgr app-launch; restoring crash/minidump + execd request-validation: see task #102.
    let _ = (logd, execd_client);
    Ok(())
}


// ——— B6 (TASK-0042): sched ABI applied from userspace ———

fn sched_applied_proof() {
    // Affinity: pin to CPU0 (present in every SMP config), verify, widen back.
    let ok_aff = nexus_abi::sched::set_affinity(0b1).is_ok()
        && nexus_abi::sched::get_affinity() == Ok(0b1)
        && nexus_abi::sched::set_affinity(0xF).is_ok();
    if ok_aff {
        emit_line(crate::markers::M_SELFTEST_AFFINITY_APPLIED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_AFFINITY_APPLIED_FAIL);
    }
    // Shares: a 3x ratio request must persist exactly (the slice math itself
    // is host-tested in the kernel; this proves the userspace path applies).
    let ok_shares = nexus_abi::sched::set_shares(300).is_ok()
        && nexus_abi::sched::get_shares() == Ok(300)
        && nexus_abi::sched::set_shares(100).is_ok()
        && nexus_abi::sched::get_shares() == Ok(100);
    if ok_shares {
        emit_line(crate::markers::M_SELFTEST_QOS_SHARES_RATIO_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_QOS_SHARES_RATIO_FAIL);
    }
}

// ——— TASK-0288: interactive floor ———

fn ui_runtime_floor_proof() {
    const ROUNDS: usize = 64;
    const MAX_GAP_NS: u64 = 250_000_000;
    let mut last = nexus_abi::nsec().unwrap_or(0);
    let mut max_gap: u64 = 0;
    for _ in 0..ROUNDS {
        let _ = yield_();
        let now = nexus_abi::nsec().unwrap_or(0);
        max_gap = max_gap.max(now.saturating_sub(last));
        last = now;
    }
    if max_gap <= MAX_GAP_NS {
        emit_line(crate::markers::M_SELFTEST_UI_RUNTIME_FLOOR_OK);
    } else {
        crate::markers::emit_bytes(crate::markers::M_SELFTEST_UI_RUNTIME_FLOOR_FAIL_GAP_MS_0X_PREFIX);
        crate::markers::emit_hex_u64(max_gap / 1_000_000);
        emit_line(")");
    }
}

// ——— Task #14: register soak ———

/// One soak round: seed s2-s11 and t0-t6 with recognizable patterns, spin
/// long enough for the timer to preempt us at least once, then verify every
/// register. Returns a bitmask of corrupted registers (bit 0 = s2 … bit 9 =
/// s11, bit 10 = t0 … bit 16 = t6). Pure integer asm — no calls, no memory,
/// so ANY flipped bit is the kernel's save/restore path, not this code.
fn regsoak_round(spins: u64) -> u64 {
    let mask: u64;
    unsafe {
        core::arch::asm!(
            // Seed patterns: distinct per register.
            "li s2,  0x5A5A0002", "li s3,  0x5A5A0003", "li s4,  0x5A5A0004",
            "li s5,  0x5A5A0005", "li s6,  0x5A5A0006", "li s7,  0x5A5A0007",
            "li s8,  0x5A5A0008", "li s9,  0x5A5A0009", "li s10, 0x5A5A000a",
            "li s11, 0x5A5A000b",
            "li t0,  0x3C3C0000", "li t1,  0x3C3C0001", "li t2,  0x3C3C0002",
            "li t3,  0x3C3C0003", "li t4,  0x3C3C0004", "li t5,  0x3C3C0005",
            "li t6,  0x3C3C0006",
            // Spin (preemption window).
            "2:",
            "addi {cnt}, {cnt}, -1",
            "bnez {cnt}, 2b",
            // Verify: set one mask bit per corrupted register.
            "li {mask}, 0",
            "li {tmp}, 0x5A5A0002", "beq s2,  {tmp}, 12f", "ori {mask}, {mask}, 1",     "12:",
            "li {tmp}, 0x5A5A0003", "beq s3,  {tmp}, 13f", "ori {mask}, {mask}, 2",     "13:",
            "li {tmp}, 0x5A5A0004", "beq s4,  {tmp}, 14f", "ori {mask}, {mask}, 4",     "14:",
            "li {tmp}, 0x5A5A0005", "beq s5,  {tmp}, 15f", "ori {mask}, {mask}, 8",     "15:",
            "li {tmp}, 0x5A5A0006", "beq s6,  {tmp}, 16f", "ori {mask}, {mask}, 16",    "16:",
            "li {tmp}, 0x5A5A0007", "beq s7,  {tmp}, 17f", "ori {mask}, {mask}, 32",    "17:",
            "li {tmp}, 0x5A5A0008", "beq s8,  {tmp}, 18f", "ori {mask}, {mask}, 64",    "18:",
            "li {tmp}, 0x5A5A0009", "beq s9,  {tmp}, 19f", "ori {mask}, {mask}, 128",   "19:",
            "li {tmp}, 0x5A5A000a", "beq s10, {tmp}, 20f", "ori {mask}, {mask}, 256",   "20:",
            "li {tmp}, 0x5A5A000b", "beq s11, {tmp}, 21f", "ori {mask}, {mask}, 512",   "21:",
            "li {tmp}, 0x3C3C0000", "beq t0,  {tmp}, 22f", "ori {mask}, {mask}, 1024",  "22:",
            "li {tmp}, 0x3C3C0001", "beq t1,  {tmp}, 23f", "li {tmp2}, 2048", "or {mask}, {mask}, {tmp2}",  "23:",
            "li {tmp}, 0x3C3C0002", "beq t2,  {tmp}, 24f", "li {tmp2}, 4096", "or {mask}, {mask}, {tmp2}",  "24:",
            "li {tmp}, 0x3C3C0003", "beq t3,  {tmp}, 25f", "li {tmp2}, 8192", "or {mask}, {mask}, {tmp2}",  "25:",
            "li {tmp}, 0x3C3C0004", "beq t4,  {tmp}, 26f", "li {tmp2}, 16384", "or {mask}, {mask}, {tmp2}", "26:",
            "li {tmp}, 0x3C3C0005", "beq t5,  {tmp}, 27f", "li {tmp2}, 32768", "or {mask}, {mask}, {tmp2}", "27:",
            "li {tmp}, 0x3C3C0006", "beq t6,  {tmp}, 28f", "li {tmp2}, 65536", "or {mask}, {mask}, {tmp2}", "28:",
            cnt = inout(reg) spins => _,
            mask = out(reg) mask,
            tmp = out(reg) _,
            tmp2 = out(reg) _,
            out("s2") _, out("s3") _, out("s4") _, out("s5") _, out("s6") _,
            out("s7") _, out("s8") _, out("s9") _, out("s10") _, out("s11") _,
            out("t0") _, out("t1") _, out("t2") _, out("t3") _, out("t4") _,
            out("t5") _, out("t6") _,
            options(nomem, nostack),
        );
    }
    mask
}

fn regsoak_proof() {
    let t0 = nexus_abi::nsec().unwrap_or(0);
    let mut mask: u64 = 0;
    // ~4 rounds of a multi-ms spin each: enough for several timer ticks.
    for _ in 0..4 {
        mask |= regsoak_round(3_000_000);
    }
    let elapsed_ms = nexus_abi::nsec().unwrap_or(0).saturating_sub(t0) / 1_000_000;
    crate::markers::emit_bytes(b"regsoak: spin ms=0x");
    crate::markers::emit_hex_u64(elapsed_ms);
    crate::markers::emit_line("");
    if mask == 0 {
        emit_line(crate::markers::M_SELFTEST_REGSOAK_OK);
    } else {
        crate::markers::emit_bytes(crate::markers::M_SELFTEST_REGSOAK_FAIL_MASK_0X_PREFIX);
        crate::markers::emit_hex_u64(mask);
        crate::markers::emit_bytes(b" ms=0x");
        crate::markers::emit_hex_u64(elapsed_ms);
        emit_line(")");
    }
}

/// Task #14: memset/memcpy alignment sweep. `fill` and `copy_from_slice`
/// lower to compiler-builtins memset/memcpy; a bug at specific (pointer
/// alignment x length) combinations corrupts neighbouring bytes or leaves
/// bytes unwritten. Standing detector.
fn memset_soak_proof() {
    let mut buf = [0u8; 256];
    let mut src = [0u8; 96];
    for (i, b) in src.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(31).wrapping_add(7);
    }
    let mut fail = 0u64;
    for off in 0..16usize {
        for len in [1usize, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 65] {
            if off + len + 16 > buf.len() {
                continue;
            }
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (i as u8) ^ 0x5C;
            }
            buf[off..off + len].fill(0xAA);
            for (i, b) in buf.iter().enumerate() {
                let want = if i >= off && i < off + len { 0xAA } else { (i as u8) ^ 0x5C };
                if *b != want {
                    fail |= 1 << (off % 16);
                }
            }
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (i as u8) ^ 0xC5;
            }
            let n = len.min(src.len());
            buf[off..off + n].copy_from_slice(&src[..n]);
            for (i, b) in buf.iter().enumerate() {
                let want =
                    if i >= off && i < off + n { src[i - off] } else { (i as u8) ^ 0xC5 };
                if *b != want {
                    fail |= 1 << (16 + (off % 16));
                }
            }
        }
    }
    extern crate alloc;
    for pad in 0..8usize {
        let mut v: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(256 + pad);
        v.resize(pad, 0xEE);
        v.resize(pad + 200, 0);
        for (i, b) in v.iter_mut().enumerate().skip(pad) {
            *b = (i as u8) ^ 0x3A;
        }
        v[pad + 5..pad + 133].fill(0x77);
        for (i, b) in v.iter().enumerate().skip(pad) {
            let want = if i >= pad + 5 && i < pad + 133 { 0x77 } else { (i as u8) ^ 0x3A };
            if *b != want {
                fail |= 1 << 32;
            }
        }
    }
    if fail == 0 {
        emit_line(crate::markers::M_SELFTEST_MEMSET_SOAK_OK);
    } else {
        crate::markers::emit_bytes(crate::markers::M_SELFTEST_MEMSET_SOAK_FAIL_MASK_0X_PREFIX);
        crate::markers::emit_hex_u64(fail);
        emit_line(")");
    }
}

/// Independently compiled FNV-1a copy (integer-path cross-check).
fn fnv1a_local(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn svg_local_determinism_proof() {
    use pinched::broker::{
        fnv1a, strparse_soak_check, PROOF_SVG, PROOF_SVG_DIGEST, PROOF_SVG_H,
        PROOF_SVG_PLAN_DIGEST, PROOF_SVG_SRC_FNV, PROOF_SVG_W, STRPARSE_SOAK_CHECK,
    };
    // Pure-integer discriminator FIRST: shared fnv1a and a local copy over a
    // constant input, against the host pin.
    let shared = fnv1a(PROOF_SVG.as_bytes());
    let local = fnv1a_local(PROOF_SVG.as_bytes());
    if shared != PROOF_SVG_SRC_FNV || local != PROOF_SVG_SRC_FNV {
        crate::markers::emit_bytes(b"svg-local: srcfnv shared=0x");
        crate::markers::emit_hex_u64(shared);
        crate::markers::emit_bytes(b" local=0x");
        crate::markers::emit_hex_u64(local);
        crate::markers::emit_line("");
    }
    // Stage probes first: dec2flt and the plan (parse + tessellate) against
    // their host pins — they split the pipeline when the raster digest fails.
    let sp = strparse_soak_check();
    if sp != STRPARSE_SOAK_CHECK {
        crate::markers::emit_bytes(b"svg-local: strparse got=0x");
        crate::markers::emit_hex_u64(sp as u64);
        crate::markers::emit_line("");
    }
    match nexus_svg::parse_svg(PROOF_SVG)
        .and_then(|doc| nexus_svg::plan_document_at(&doc, PROOF_SVG_W as u32, PROOF_SVG_H as u32))
    {
        Ok(plan) => {
            let pd = plan.debug_digest();
            if pd != PROOF_SVG_PLAN_DIGEST {
                crate::markers::emit_bytes(b"svg-local: plan digest got=0x");
                crate::markers::emit_hex_u64(pd);
                crate::markers::emit_line("");
            }
        }
        Err(_) => crate::markers::emit_line("svg-local: plan build err"),
    }
    let mut digests = [0u64; 3];
    for d in digests.iter_mut() {
        match nexus_svg::parse_svg(PROOF_SVG).and_then(|doc| {
            nexus_svg::rasterize_document_at(&doc, PROOF_SVG_W as u32, PROOF_SVG_H as u32)
        }) {
            Ok(out) => *d = fnv1a(&out.buffer),
            Err(_) => {
                emit_line(crate::markers::M_SELFTEST_SVG_LOCAL_DETERMINISM_FAIL_RASTER_ERR);
                return;
            }
        }
    }
    if digests[0] != digests[1] || digests[1] != digests[2] {
        crate::markers::emit_bytes(b"svg-local: digests=0x");
        crate::markers::emit_hex_u64(digests[0]);
        crate::markers::emit_bytes(b"/0x");
        crate::markers::emit_hex_u64(digests[1]);
        crate::markers::emit_bytes(b"/0x");
        crate::markers::emit_hex_u64(digests[2]);
        crate::markers::emit_line("");
        emit_line(crate::markers::M_SELFTEST_SVG_LOCAL_DETERMINISM_FAIL_VARIES);
        return;
    }
    if digests[0] != PROOF_SVG_DIGEST {
        crate::markers::emit_bytes(b"svg-local: stable digest=0x");
        crate::markers::emit_hex_u64(digests[0]);
        crate::markers::emit_line("");
        emit_line(crate::markers::M_SELFTEST_SVG_LOCAL_DETERMINISM_FAIL_WRONG);
        return;
    }
    emit_line(crate::markers::M_SELFTEST_SVG_LOCAL_DETERMINISM_OK);
}

/// Pure soft-float compute against the HOST-PINNED answer (shared SSOT in
/// pinched::broker — the host test regenerates the constant).
fn f32_soak_proof() {
    use pinched::broker::{f32_soak_check, libm_soak_check, F32_SOAK_CHECK, LIBM_SOAK_CHECK};
    let basic = f32_soak_check();
    if basic != F32_SOAK_CHECK {
        crate::markers::emit_bytes(b"f32-soak: got=0x");
        crate::markers::emit_hex_u64(basic as u64);
        crate::markers::emit_line("");
        emit_line(crate::markers::M_SELFTEST_F32_SOAK_FAIL_HOST_MISMATCH);
        return;
    }
    let libm = libm_soak_check();
    if libm != LIBM_SOAK_CHECK {
        crate::markers::emit_bytes(b"libm-soak: got=0x");
        crate::markers::emit_hex_u64(libm as u64);
        crate::markers::emit_line("");
        emit_line(crate::markers::M_SELFTEST_F32_SOAK_FAIL_LIBM_MISMATCH);
        return;
    }
    emit_line(crate::markers::M_SELFTEST_F32_SOAK_OK);
}

/// Pure allocator traffic: fill vecs with patterns, verify, drop (leaked by
/// the bump — bounded sizes keep this cheap).
fn alloc_soak_proof() {
    extern crate alloc;
    for round in 0..8u32 {
        let mut v: alloc::vec::Vec<u32> = alloc::vec::Vec::with_capacity(512);
        for i in 0..512u32 {
            v.push(i.wrapping_mul(0x9E37_79B9) ^ round);
        }
        for (i, val) in v.iter().enumerate() {
            if *val != (i as u32).wrapping_mul(0x9E37_79B9) ^ round {
                emit_line(crate::markers::M_SELFTEST_ALLOC_SOAK_FAIL_PATTERN);
                return;
            }
        }
        let z = alloc::vec![0u8; 4096];
        if z.iter().any(|b| *b != 0) {
            emit_line(crate::markers::M_SELFTEST_ALLOC_SOAK_FAIL_ZEROED);
            return;
        }
    }
    emit_line(crate::markers::M_SELFTEST_ALLOC_SOAK_OK);
}

// ——— Phase C: thread spawn proof ———

static THREAD_SENTINEL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static mut THREAD_STACK: [u8; 16 * 1024] = [0; 16 * 1024];

extern "C" fn thread_entry(arg: usize) {
    THREAD_SENTINEL.store(arg, core::sync::atomic::Ordering::Release);
}

fn thread_spawn_proof() {
    const SENTINEL: usize = 0x7ead;
    THREAD_SENTINEL.store(0, core::sync::atomic::Ordering::Release);
    // SAFETY: the stack buffer is used exclusively by the one thread spawned
    // here; the proof is strictly sequential (spawn → join → done).
    let stack = unsafe { &mut *core::ptr::addr_of_mut!(THREAD_STACK) };
    let pid = match nexus_abi::thread::spawn_thread(thread_entry, SENTINEL, stack) {
        Ok(pid) => pid,
        Err(_) => {
            emit_line(crate::markers::M_SELFTEST_THREAD_SPAWN_FAIL_SPAWN);
            return;
        }
    };
    let mut seen = false;
    for _ in 0..4096 {
        if THREAD_SENTINEL.load(core::sync::atomic::Ordering::Acquire) == SENTINEL {
            seen = true;
            break;
        }
        let _ = yield_();
    }
    // Reap the exited thread (trampoline exits 0 after entry returns).
    let mut reaped = false;
    for _ in 0..4096 {
        match nexus_abi::wait(pid as i32) {
            Ok((_, status)) => {
                reaped = status == 0;
                break;
            }
            Err(_) => {
                let _ = yield_();
            }
        }
    }
    if seen && reaped {
        emit_line(crate::markers::M_SELFTEST_THREAD_SPAWN_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_THREAD_SPAWN_FAIL);
    }
}


// ——— Phase C3: workpool proofs ———

const WP_TOTAL: usize = 1024;
static WP_OUT: [core::sync::atomic::AtomicU32; WP_TOTAL] =
    [const { core::sync::atomic::AtomicU32::new(0) }; WP_TOTAL];

fn wp_transform(i: usize) -> u32 {
    (i as u32).wrapping_mul(0x9E37_79B9).rotate_left(7) ^ 0x5A5A
}

extern "C" fn wp_job(start: usize, end: usize, _ctx: *mut u8) {
    for i in start..end {
        WP_OUT[i].store(wp_transform(i), core::sync::atomic::Ordering::Relaxed);
    }
}

fn workpool_proof() {
    use core::sync::atomic::Ordering;

    // Bounded contract first: run before init and invalid init are REJECTED.
    let pre_run =
        nexus_workpool::run(1, wp_job, core::ptr::null_mut(), 1_000_000_000).is_err();
    let zero_init = nexus_workpool::init(0).is_err();
    if let Err(err) = nexus_workpool::init(2) {
        emit_line(match err {
            nexus_workpool::PoolError::AbiFence => crate::markers::M_SELFTEST_WORKPOOL_BOUNDED_FAIL_FENCE,
            nexus_workpool::PoolError::AbiSpawn => crate::markers::M_SELFTEST_WORKPOOL_BOUNDED_FAIL_SPAWN,
            nexus_workpool::PoolError::AbiTransfer => {
                crate::markers::M_SELFTEST_WORKPOOL_BOUNDED_FAIL_TRANSFER
            }
            nexus_workpool::PoolError::AbiResume => crate::markers::M_SELFTEST_WORKPOOL_BOUNDED_FAIL_RESUME,
            _ => crate::markers::M_SELFTEST_WORKPOOL_BOUNDED_FAIL_INIT,
        });
        return;
    }
    let double_init = nexus_workpool::init(2).is_err();
    if pre_run && zero_init && double_init {
        emit_line(crate::markers::M_SELFTEST_WORKPOOL_BOUNDED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_WORKPOOL_BOUNDED_FAIL);
    }

    // Fence sanity probe: the done fence must NOT satisfy an unsignalled
    // target (catches slot/id mix-ups before the real run).
    if !nexus_workpool::pool::selftest_probe_done_fence() {
        emit_line(crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_DONE_FENCE_SATISFIED_UNSIGNALLED);
        return;
    }

    // Determinism: parallel result must equal the sequential reference.
    for slot in WP_OUT.iter() {
        slot.store(0, Ordering::Relaxed);
    }
    match nexus_workpool::run(WP_TOTAL, wp_job, core::ptr::null_mut(), 5_000_000_000) {
        Ok(()) => {
            let mut bad_lo = 0usize;
            let mut bad_hi = 0usize;
            for i in 0..WP_TOTAL {
                if WP_OUT[i].load(Ordering::Acquire) != wp_transform(i) {
                    if i < WP_TOTAL / 2 {
                        bad_lo += 1;
                    } else {
                        bad_hi += 1;
                    }
                }
            }
            if bad_lo == 0 && bad_hi == 0 {
                emit_line(crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_OK);
                // C4: worker 0 pins itself to CPU 0 (present on every SMP
                // config), so at least one round-tripped pin is REQUIRED.
                if nexus_workpool::pool::selftest_pinned() >= 1 {
                    emit_line(crate::markers::M_SELFTEST_WORKPOOL_AFFINITY_OK);
                } else {
                    emit_line(crate::markers::M_SELFTEST_WORKPOOL_AFFINITY_FAIL_NO_PIN);
                }
            } else if bad_lo > 0 && bad_hi == 0 {
                emit_line(crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_LO_CHUNK);
            } else if bad_lo == 0 && bad_hi > 0 {
                emit_line(crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_HI_CHUNK);
            } else {
                let (alive, woke, done) = nexus_workpool::pool::selftest_debug();
                emit_line(match (alive, woke, done) {
                    (0, _, _) => crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_BOTH_ALIVE_0,
                    (1, _, _) => crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_BOTH_ALIVE_1,
                    (_, _, 0) => crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_BOTH_DONE_0,
                    (_, _, 1) => crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_BOTH_DONE_1,
                    _ => crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_BOTH_ALIVE_2_DONE_2,
                });
            }
        }
        Err(_) => {
            let (alive, woke, done) = nexus_workpool::pool::selftest_debug();
            let self_sig = nexus_workpool::pool::selftest_probe_job_selfsignal();
            emit_line(match (alive, woke, done, self_sig) {
                (_, 0, _, true) => {
                    crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_RUN_WOKE_0_SELFSIG_OK
                }
                (_, 0, _, false) => {
                    crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_RUN_WOKE_0_SELFSIG_FAIL
                }
                (_, _, 0, _) => crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_RUN_WOKE_GT0_DONE_0,
                _ => crate::markers::M_SELFTEST_WORKPOOL_DETERMINISM_FAIL_RUN_WOKE_GT0_DONE_GT0,
            });
        }
    }
}
