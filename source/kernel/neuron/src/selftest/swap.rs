// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot-time proof of the swap stage (TASK-0333 6f-2, ADR-0088, RFC-0100 §5) against the
//! real syscall table, the fault's own fill and the pressure sink. Selftest tasks play the parts:
//! an app whose lazy object holds three squeezed text pages, the RNG service (its kernel
//! identity), the pager (the sink's holder), a stranger, a waiter and a victim. The refusals
//! first — a key from anyone but the RNG service, an export by a stranger, an export without a
//! key, a second key. Then the export, twice (the first superseded), and the commits: a
//! stranger's and the superseded export's are refused, the rest make the pages swapped and
//! release the records. The verdicts: the fault's fill and a syscall over a swapped page both
//! name its token and slot. The park: a waiter parks on page 0, a second ask adds no request,
//! the pager hears exactly one `OP_PAGE_IN`. The page-in refusals — a stranger, a stale slot, a
//! tampered record, the superseded export's record replayed — then the answer: the waiter woken,
//! the page back byte-exact. A victim parked on page 1 is killed by a record that does not open.
//! Last the object's death: the pager hears the slots of the pages still swapped. The boot
//! proof's key is removed and the sink cleared at the end, whatever the outcome — the RNG
//! service's seed and memd's bind find both free. A child of `pressure`, beside the squeeze.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: swap ok (…)`, every QEMU lane and the board ladder

use alloc::vec::Vec;

use super::squeeze::{page_bytes, record};
use crate::cap::{CapError, Capability, CapabilityKind, Rights};
use crate::lazy::seal::KEY_LEN;
use crate::lazy::swap as wire;
use crate::lazy::Access;
use crate::mm::populate::{self, FillError};
use crate::mm::{pressure_endpoint, vmo, PAGE_SIZE};
use crate::pressure;
use crate::selftest::Context;
use crate::syscall::{
    api, Args, Error as SysError, SyscallTable, SYSCALL_MEM_PRESSURE_BIND, SYSCALL_MM_STATS,
    SYSCALL_SWAP_COMMIT, SYSCALL_SWAP_EXPORT, SYSCALL_SWAP_KEY, SYSCALL_SWAP_PAGE_IN,
};
use crate::task::{BlockReason, Pid, TaskState};

/// The app's object: `PAGES` pages here, the first `TEXT` written with text.
const OWN_VA: usize = 0x0010_0000;
const PAGES: usize = 4;
const TEXT: usize = 3;
/// The RNG service's key page.
const KEY_VA: usize = 0x1000;
/// The pager's export buffer.
const BUF_VA: usize = 0x0040_0000;
const BUF_PAGES: usize = wire::EXPORT_MAX_BUF.div_ceil(PAGE_SIZE);
/// Bootstrap cap slot above the pressure selftest's (56).
const SINK_SLOT: usize = 57;
/// The boot proof's key: compiled in, so it is removed before any real record could meet it.
const KEY: [u8; KEY_LEN] = *b"boot proof key, never a real one";
/// The pager's slots for the three pages.
const SLOT: u32 = 100;

/// The byte at `at` of text page `page`.
fn pattern(page: usize, at: usize) -> u8 {
    const PHRASE: &[u8] = b"a frozen app's pages leave sealed and come back when touched. ";
    PHRASE[(at + page * 11) % PHRASE.len()]
}

/// One entry of an export buffer, as the pager would keep it.
#[derive(Clone)]
struct Entry {
    token: u64,
    ctr: u64,
    sealed: Vec<u8>,
}

/// The first `len` bytes of committed object `id`, page by page.
fn object_bytes(id: u32, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    for page in 0..len.div_ceil(PAGE_SIZE) {
        let Some(bytes) = page_bytes(id, page) else { break };
        out.extend_from_slice(&bytes[..(len - out.len()).min(PAGE_SIZE)]);
    }
    out
}

/// The entries of an export buffer; `None` when it is not one.
fn entries(buf: &[u8]) -> Option<Vec<Entry>> {
    let word = |at: usize| Some(u32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?));
    let long = |at: usize| Some(u64::from_le_bytes(buf.get(at..at + 8)?.try_into().ok()?));
    if word(0)? != wire::EXPORT_VERSION {
        return None;
    }
    let mut out = Vec::new();
    let mut at = wire::EXPORT_HEADER;
    for _ in 0..word(4)? {
        let len = usize::from(u16::from_le_bytes(buf.get(at + 16..at + 18)?.try_into().ok()?));
        let sealed = buf.get(at + wire::ENTRY_HEAD..at + wire::ENTRY_HEAD + len)?.to_vec();
        out.push(Entry { token: long(at)?, ctr: long(at + 8)?, sealed });
        at += wire::entry_len(len);
    }
    Some(out)
}

fn call(
    table: &SyscallTable,
    sys: &mut api::Context<'_>,
    number: usize,
    args: [usize; 4],
) -> Result<usize, SysError> {
    table.dispatch(number, sys, &Args::new([args[0], args[1], args[2], args[3], 0, 0]))
}

/// Every record of `app` exported, batch by batch, as the pager reads them out of its buffer.
fn export_all(
    table: &SyscallTable,
    sys: &mut api::Context<'_>,
    app: Pid,
    buf: u32,
) -> Option<Vec<Entry>> {
    let mut all = Vec::new();
    let mut from = 0usize;
    for _ in 0..16 {
        let args = [app.as_raw() as usize, from, BUF_VA, wire::EXPORT_MAX_BUF];
        let next = call(table, sys, SYSCALL_SWAP_EXPORT, args).ok()?;
        all.extend(entries(&object_bytes(buf, wire::EXPORT_MAX_BUF))?);
        if next == 0 {
            return Some(all);
        }
        from = next;
    }
    None
}

/// The one frame waiting on the pager's sink, if any.
fn heard(router: &mut crate::ipc::Router, ep: u32) -> Option<Vec<u8>> {
    router.recv(ep).ok().map(|msg| msg.payload.as_slice().to_vec())
}

pub(in crate::selftest) fn run_swap_selftest(ctx: &mut Context<'_>) {
    use crate::hal::Timer;
    let clock = ctx.hal.timer();
    let started = clock.now();
    let fail = |step: &str| log_error!(target: "selftest", "KSELFTEST: swap FAIL ({})", step);
    // Never under a real key or a real pager (the selftests run before userspace; a held one is
    // a boot-order break, named, and left alone).
    if crate::mm::swap::has_key() || pressure_endpoint::binding().is_some() {
        return fail("key or sink held");
    }
    ctx.tasks.set_current(Pid::KERNEL);
    let parent = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let app = ctx.tasks.selftest_create_dummy_task(parent, ctx.scheduler);
    let [rng, pager, stranger, waiter] =
        core::array::from_fn(|_| ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler));
    let victim = ctx.tasks.selftest_create_dummy_task(parent, ctx.scheduler);
    if let Some(task) = ctx.tasks.task_mut(rng) {
        task.set_service_id(api::service_id_from_name(b"rngd"));
    }
    let reach = crate::dma_reach::DmaReach::ALL;
    let lazy = |pages: usize| vmo::create(pages * PAGE_SIZE, vmo::VmoKind::Lazy, &reach);
    let (Ok(own), Ok(key), Ok(buf)) = (lazy(PAGES), lazy(1), lazy(BUF_PAGES)) else {
        return fail("create");
    };
    let spaces = &mut *ctx.address_spaces;
    let (Ok(app_as), Ok(rng_as), Ok(pager_as)) =
        (spaces.create(), spaces.create(), spaces.create())
    else {
        return fail("space");
    };
    let mut ready = record(spaces, app_as, OWN_VA, PAGES * PAGE_SIZE, own)
        && record(spaces, rng_as, KEY_VA, PAGE_SIZE, key)
        && record(spaces, pager_as, BUF_VA, BUF_PAGES * PAGE_SIZE, buf);
    for (pid, handle) in [(app, app_as), (rng, rng_as), (pager, pager_as)] {
        ready &= ctx.address_spaces.attach(handle, pid).is_ok();
        if let Some(task) = ctx.tasks.task_mut(pid) {
            task.address_space = Some(handle);
        }
    }
    // The app's text and the key, written through committed pages.
    ready &= ctx.address_spaces.get_mut(app_as).is_ok_and(|space| {
        (0..TEXT).all(|i| populate::fill(space, OWN_VA + i * PAGE_SIZE, Access::Write).is_ok())
    }) && ctx
        .address_spaces
        .get_mut(rng_as)
        .is_ok_and(|space| populate::fill(space, KEY_VA, Access::Write).is_ok());
    for i in 0..TEXT {
        match page_bytes(own, i) {
            Some(bytes) => bytes.iter_mut().enumerate().for_each(|(at, b)| *b = pattern(i, at)),
            None => ready = false,
        }
    }
    match page_bytes(key, 0) {
        Some(bytes) => bytes[..KEY_LEN].copy_from_slice(&KEY),
        None => ready = false,
    }
    // The pager's sink.
    let sink = ctx.router.create_endpoint(4, Some(pager.as_raw())).ok();
    let cap = sink.map(|ep| Capability {
        kind: CapabilityKind::Endpoint(ep),
        rights: Rights::RECV | Rights::SEND,
    });
    ready &= cap.is_some_and(|cap| {
        ctx.tasks.caps_of_mut(pager).is_some_and(|caps| caps.set(SINK_SLOT, cap).is_ok())
    });
    let sink = sink.unwrap_or(0);
    // The app's text squeezed into records (the squeeze's own proof is `task squeeze ok`).
    let (base_squeezed, _, base_frames) = vmo::squeeze::stats();
    let mut batch = crate::mm::squeeze::Batch::default();
    if let Ok(space) = ctx.address_spaces.get_mut(app_as) {
        let mut from = 0;
        for _ in 0..8 {
            from = crate::mm::squeeze::run(space, from, &[own], &mut batch);
            if from == 0 {
                break;
            }
        }
    }
    ready &= batch.squeezed == TEXT as u64;

    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );
    let sys = &mut sys_ctx;
    let denied = Err(SysError::Capability(CapError::PermissionDenied));
    let invalid = Err(SysError::AddressSpace(crate::mm::AddressSpaceError::InvalidArgs));
    let stale = Err(SysError::InvalidTarget);
    let export_args = [app.as_raw() as usize, 0, BUF_VA, wire::EXPORT_MAX_BUF];

    // The pager binds the sink; the level of the moment is drained.
    sys.tasks.set_current(pager);
    let bound = call(&table, sys, SYSCALL_MEM_PRESSURE_BIND, [SINK_SLOT, 0, 0, 0]) == Ok(0);
    pressure_endpoint::dispatch(sys.router, sys.tasks, sys.scheduler);
    let leveled = heard(sys.router, sink).is_some_and(|f| pressure::decode(&f).is_some());

    // The key and the export refusals: a stranger's key, a stranger's export, no key yet.
    sys.tasks.set_current(stranger);
    let mut reject = call(&table, sys, SYSCALL_SWAP_KEY, [KEY_VA, KEY_LEN, 0, 0]) == denied;
    reject &= call(&table, sys, SYSCALL_SWAP_EXPORT, export_args) == denied;
    sys.tasks.set_current(pager);
    reject &= call(&table, sys, SYSCALL_SWAP_EXPORT, export_args) == invalid;
    // The RNG service's key, once.
    sys.tasks.set_current(rng);
    let keyed = call(&table, sys, SYSCALL_SWAP_KEY, [KEY_VA, KEY_LEN, 0, 0]) == Ok(0);
    reject &=
        call(&table, sys, SYSCALL_SWAP_KEY, [KEY_VA, KEY_LEN, 0, 0]) == Err(SysError::ResourceBusy);

    // Two exports: the second supersedes the first; the pages stay records until committed.
    sys.tasks.set_current(pager);
    let first = export_all(&table, sys, app, buf).unwrap_or_default();
    let second = export_all(&table, sys, app, buf).unwrap_or_default();
    let exported = first.len() == TEXT
        && second.len() == TEXT
        && first.iter().zip(&second).all(|(a, b)| a.token == b.token && a.ctr != b.ctr)
        && vmo::squeeze::stats().0 == base_squeezed + TEXT as u64;
    let sealed: usize = second.iter().map(|e| e.sealed.len()).sum();
    let token = |i: usize| second.get(i).map_or(0, |e| e.token);
    let commit = |e: &Entry, slot: u32| [e.token as usize, e.ctr as usize, slot as usize, 0];

    // The commits: a stranger's and the superseded export's refused, then every page swapped.
    let base_swapped = vmo::swap::swapped();
    let mut committed = exported;
    if let (Some(old), Some(new)) = (first.first(), second.first()) {
        sys.tasks.set_current(stranger);
        reject &= call(&table, sys, SYSCALL_SWAP_COMMIT, commit(new, SLOT)) == denied;
        sys.tasks.set_current(pager);
        reject &= call(&table, sys, SYSCALL_SWAP_COMMIT, commit(old, SLOT)) == stale;
    }
    for (i, entry) in second.iter().enumerate() {
        committed &=
            call(&table, sys, SYSCALL_SWAP_COMMIT, commit(entry, SLOT + i as u32)) == Ok(0);
    }
    let (after_squeezed, _, after_frames) = vmo::squeeze::stats();
    committed &= vmo::swap::swapped() == base_swapped + TEXT as u64
        && after_squeezed == base_squeezed
        && after_frames == base_frames;

    // The verdicts: the fault's fill and a syscall over a swapped page name its token and slot.
    let fault_says = sys.address_spaces.get_mut(app_as).is_ok_and(|space| {
        populate::fill(space, OWN_VA, Access::Read)
            == Err(FillError::PageIn { token: token(0), slot: SLOT })
    });
    sys.tasks.set_current(app);
    let stats_len = crate::accounting::MM_STATS_BYTES;
    let syscall_says = call(&table, sys, SYSCALL_MM_STATS, [OWN_VA + PAGE_SIZE, stats_len, 0, 0])
        == Err(SysError::PageIn { token: token(1), slot: SLOT + 1 });

    // The park: one request however many ask.
    sys.tasks.set_current(waiter);
    let asked = crate::mm::swap::ask(token(0), SLOT);
    sys.tasks.block_current(BlockReason::PageIn { token: token(0) }, sys.scheduler);
    let asked_again = crate::mm::swap::ask(token(0), SLOT);
    pressure_endpoint::dispatch(sys.router, sys.tasks, sys.scheduler);
    let request = heard(sys.router, sink).and_then(|f| pressure::decode_page_in(&f));
    let once = heard(sys.router, sink).is_none();
    let parked = asked && asked_again && request == Some((token(0), SLOT)) && once;

    // The page-in refusals: a stranger, a stale slot, a tampered record, a replayed one.
    let mut back = false;
    let mut killed = 0usize;
    if let (Some(e0), Some(e1), Some(old1)) = (second.first(), second.get(1), first.get(1)) {
        let mut tampered = e1.sealed.clone();
        if let Some(byte) = tampered.get_mut(7) {
            *byte ^= 0x40;
        }
        // The kernel reads the record through the caller's page table: it is put into the
        // pager's buffer first, as the pager would after reading its slot.
        let put = |bytes: &[u8]| {
            page_bytes(buf, 0).map(|page| page[..bytes.len()].copy_from_slice(bytes)).is_some()
        };
        let at_buf =
            |e: &Entry, slot: u32, len: usize| [e.token as usize, slot as usize, BUF_VA, len];
        sys.tasks.set_current(stranger);
        reject &= put(&e0.sealed)
            && call(&table, sys, SYSCALL_SWAP_PAGE_IN, at_buf(e0, SLOT, e0.sealed.len())) == denied;
        sys.tasks.set_current(pager);
        reject &= put(&e1.sealed)
            && call(&table, sys, SYSCALL_SWAP_PAGE_IN, at_buf(e1, SLOT + 2, e1.sealed.len()))
                == stale;
        reject &= put(&tampered)
            && call(&table, sys, SYSCALL_SWAP_PAGE_IN, at_buf(e1, SLOT + 1, tampered.len()))
                == invalid;
        reject &= put(&old1.sealed)
            && call(&table, sys, SYSCALL_SWAP_PAGE_IN, at_buf(e1, SLOT + 1, old1.sealed.len()))
                == invalid;
        // The answer: the waiter woken, the page back byte-exact.
        let ins = vmo::swap::page_ins();
        back = put(&e0.sealed)
            && call(&table, sys, SYSCALL_SWAP_PAGE_IN, at_buf(e0, SLOT, e0.sealed.len())) == Ok(0)
            && vmo::swap::page_ins() == ins + 1
            && sys.tasks.task(waiter).is_some_and(|t| !t.is_blocked())
            && page_bytes(own, 0)
                .is_some_and(|bytes| bytes.iter().enumerate().all(|(at, b)| *b == pattern(0, at)));
        // A victim parked on page 1 dies with a record that does not open.
        sys.tasks.set_current(victim);
        let victim_asked = crate::mm::swap::ask(e1.token, SLOT + 1);
        sys.tasks.block_current(BlockReason::PageIn { token: e1.token }, sys.scheduler);
        sys.tasks.set_current(pager);
        pressure_endpoint::dispatch(sys.router, sys.tasks, sys.scheduler);
        let _ = heard(sys.router, sink);
        let refused = put(&tampered)
            && call(&table, sys, SYSCALL_SWAP_PAGE_IN, at_buf(e1, SLOT + 1, tampered.len()))
                == invalid;
        let dead = sys.tasks.task(victim).is_some_and(|t| t.state() == TaskState::Zombie);
        killed = usize::from(victim_asked && refused && dead);
    }

    // The object's death: the pager hears the slots of the pages still swapped (1 and 2).
    sys.tasks.set_current(Pid::KERNEL);
    for (handle, pid) in [(app_as, app), (rng_as, rng), (pager_as, pager)] {
        let _ = sys.address_spaces.detach(handle, pid);
        if let Some(task) = sys.tasks.task_mut(pid) {
            task.address_space = None;
        }
        let _ = sys.address_spaces.destroy(handle);
    }
    let _ = vmo::destroy(own);
    pressure_endpoint::dispatch(sys.router, sys.tasks, sys.scheduler);
    let mut slots = [0u32; pressure::DROP_BATCH];
    let drop_frame = heard(sys.router, sink);
    let dropped = drop_frame.and_then(|f| pressure::decode_drops(&f, &mut slots)).unwrap_or(0);
    let drops_right =
        slots[..dropped] == [SLOT + 1, SLOT + 2] && vmo::swap::swapped() == base_swapped;
    for id in [key, buf] {
        let _ = vmo::destroy(id);
    }

    // The sink cleared and the key removed — whatever came before; no pager, no park.
    sys.tasks.set_current(pager);
    let cleared = call(&table, sys, SYSCALL_MEM_PRESSURE_BIND, [usize::MAX, 0, 0, 0]) == Ok(0);
    sys.tasks.set_current(Pid::KERNEL);
    let unpaged = !crate::mm::swap::ask(token(2), SLOT + 2);
    crate::mm::swap::selftest_remove_key();
    let us = clock.now().saturating_sub(started) / 1000;

    let steps = [
        ready,
        bound && leveled,
        reject,
        keyed,
        exported,
        committed,
        fault_says && syscall_says,
        parked,
        back,
        killed == 1,
        drops_right,
        cleared && unpaged && !crate::mm::swap::has_key(),
    ];
    if steps.iter().all(|s| *s) {
        log_info!(
            target: "selftest",
            "KSELFTEST: swap ok (pages={} sealed={} parked=1 back=1 killed={} dropped={} us={} reject=key,sink,nokey,stale,tampered,replayed)",
            second.len(),
            sealed,
            killed,
            dropped,
            us
        );
    } else {
        let bits: [u8; 12] = core::array::from_fn(|i| u8::from(steps[i]));
        log_error!(
            target: "selftest",
            "KSELFTEST: swap FAIL (steps={:?} first={} second={} dropped={})",
            bits,
            first.len(),
            second.len(),
            dropped
        );
    }
}
