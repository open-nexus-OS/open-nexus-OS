// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The generation-arena invariant, ENFORCED on the host (TASK-0077C P2b,
//! ADR-0065): nothing emission allocates outlives its generation.
//!
//! This binary replaces the global allocator with a two-generation arena that
//! mirrors `nexus-service-entry`'s — and goes one step further: a generation
//! that is reset is filled with `0xDE`, and so is every block freed inside a
//! generation. Recycled memory is therefore never quietly re-readable: a
//! `Vec` whose buffer was reset reads a capacity of `0xDEDE…`, a pointer reads
//! as `0xDEDE…`, and the canary below fails BEFORE anything dereferences it.
//!
//! WHY THIS EXISTS: the first attempt at scoping emission booted, cut the base
//! heap's growth 66×, and printed `alloc-fail size=0x20000004d` in the same
//! log — a length read out of recycled memory. The cause was not a store
//! write; it was `self.handlers.clear()` keeping a buffer across frames, so
//! generation g+1 wrote into memory that g+2 reset underneath it. That class of
//! bug is silent on a bump that never poisons, and it does not reproduce on an
//! ordinary heap at all. The only honest gate is an allocator that makes the
//! violation impossible to miss, running the REAL apps for many frames.
//!
//! One test function, scenarios run sequentially: the arena is process-global
//! and two scopes interleaving from parallel tests would prove nothing.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use nexus_dsl_runtime::{
    FixtureEnv, FrameScope, FrameScopeGuard, IdentityLocale, NoIo, Value, View,
};

// ---------------------------------------------------------------- the arena

/// Two halves of this; plenty for any scene in the tree (peak measured on the
/// OS: 78 KiB across both regions).
const ARENA_BYTES: usize = 8 * 1024 * 1024;
const HALF: usize = ARENA_BYTES / 2;
const POISON: u8 = 0xDE;

static mut ARENA: [u8; ARENA_BYTES] = [0; ARENA_BYTES];
/// Which half is active (0/1).
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
/// Bump cursor inside the active half, as an offset.
static USED: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
/// A scope is open: allocations come from the arena.
static OPEN: AtomicBool = AtomicBool::new(false);
/// Generations opened so far — the proof counts them.
static GENERATIONS: AtomicUsize = AtomicUsize::new(0);
/// Bytes handed out by the SYSTEM heap — i.e. outside every scope. On the OS
/// this is exactly what the never-freeing base heap would keep.
static SYSTEM_BYTES: AtomicUsize = AtomicUsize::new(0);

fn arena_base() -> usize {
    core::ptr::addr_of!(ARENA) as usize
}

fn in_arena(p: *mut u8) -> bool {
    let a = p as usize;
    a >= arena_base() && a < arena_base() + ARENA_BYTES
}

/// Opens the next generation: flip, POISON the half being opened (it held the
/// frame from two frames ago), reset its cursor.
fn open_generation() {
    let next = ACTIVE.load(Ordering::Relaxed) ^ 1;
    // Poison everything the half ever held, not just the last frame's use:
    // stale readers may reach past the last cursor.
    unsafe {
        let start = arena_base() + next * HALF;
        core::ptr::write_bytes(start as *mut u8, POISON, HALF);
    }
    USED[next].store(0, Ordering::Relaxed);
    ACTIVE.store(next, Ordering::Relaxed);
    OPEN.store(true, Ordering::Relaxed);
    GENERATIONS.fetch_add(1, Ordering::Relaxed);
}

fn close_generation() {
    OPEN.store(false, Ordering::Relaxed);
}

struct PoisoningArena;

unsafe impl GlobalAlloc for PoisoningArena {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if !OPEN.load(Ordering::Relaxed) {
            SYSTEM_BYTES.fetch_add(l.size(), Ordering::Relaxed);
            return System.alloc(l);
        }
        let half = ACTIVE.load(Ordering::Relaxed);
        let base = arena_base() + half * HALF;
        let used = USED[half].load(Ordering::Relaxed);
        let cursor = base + used;
        let aligned = (cursor + l.align() - 1) & !(l.align() - 1);
        let end = aligned + l.size();
        assert!(end <= base + HALF, "harness arena half exhausted — raise ARENA_BYTES");
        USED[half].store(end - base, Ordering::Relaxed);
        aligned as *mut u8
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if !OPEN.load(Ordering::Relaxed) {
            SYSTEM_BYTES.fetch_add(l.size(), Ordering::Relaxed);
            return System.alloc_zeroed(l);
        }
        // The arena REUSES memory (poisoned, even), so zero explicitly — the
        // same rule the OS allocator needs (ADR-0065).
        let p = self.alloc(l);
        core::ptr::write_bytes(p, 0, l.size());
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if in_arena(p) {
            // Bump: no reclaim. Poison, so a use-after-free inside a
            // generation is as loud as a use-after-reset.
            core::ptr::write_bytes(p, POISON, l.size());
            return;
        }
        System.dealloc(p, l)
    }
}

#[global_allocator]
static GLOBAL: PoisoningArena = PoisoningArena;

// ------------------------------------------------------- the runtime's hook

struct EmitScope;
static EMIT_SCOPE: EmitScope = EmitScope;

impl FrameScope for EmitScope {
    fn enter(&self) -> FrameScopeGuard {
        open_generation();
        FrameScopeGuard::new(close_generation)
    }
}

// ------------------------------------------------------------- the canary

/// Touches every retained emission output the way the next frame will —
/// through its headers first, so a poisoned buffer fails an assertion instead
/// of a dereference. `0xDEDE…` as a length or capacity is unmistakable.
fn canary(view: &View, frame: usize, app: &str) {
    const SANE: usize = 1 << 20;
    for (box_id, entry) in view.handlers() {
        assert!(*box_id < SANE, "{app} frame {frame}: handler box id {box_id:#x} is poison");
        assert!(
            entry.path.capacity() < SANE && entry.path.len() <= entry.path.capacity(),
            "{app} frame {frame}: a handler path header is poison (len {:#x} cap {:#x})",
            entry.path.len(),
            entry.path.capacity()
        );
        for seg in &entry.path {
            assert!((*seg as usize) < SANE, "{app} frame {frame}: handler path segment is poison");
        }
    }
    for (box_id, _) in view.animations() {
        assert!(*box_id < SANE, "{app} frame {frame}: animation box id {box_id:#x} is poison");
    }
    let texts = common::scene_texts(view);
    for (i, text) in texts.iter().enumerate() {
        assert!(text.len() < SANE, "{app} frame {frame}: a scene text header is poison");
        // `all()` over an EMPTY string is vacuously true — an empty text field
        // is not poison. Caught by the differential (`ARENA_OFF=1` failed the
        // same way), which is why the differential exists.
        assert!(
            text.is_empty() || !text.as_bytes().iter().all(|b| *b == POISON),
            "{app} frame {frame}: scene text #{i} of {} is poison (len {}); neighbours: {:?}",
            texts.len(),
            text.len(),
            texts
                .iter()
                .enumerate()
                .filter(|(j, _)| j.abs_diff(i) <= 1 && *j != i)
                .map(|(_, t)| t.chars().take(24).collect::<String>())
                .collect::<Vec<_>>()
        );
    }
}

/// Drives `app` through `frames` structural interactions with emission in the
/// poisoning arena, laying out after each like app-host does.
fn drive(app: &str, event: &str, cases: &[(&str, Vec<Value>)], frames: usize) {
    let nxir = common::compile(app);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(&nxir);
    let keys = common::program_i18n_keys(&nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    if std::env::var_os("ARENA_OFF").is_none() {
        view.set_frame_scope(&EMIT_SCOPE);
    }
    let mut host = NoIo;
    let before = GENERATIONS.load(Ordering::Relaxed);
    for frame in 0..frames {
        let (case, payload) = &cases[frame % cases.len()];
        let sys0 = SYSTEM_BYTES.load(Ordering::Relaxed);
        common::dispatch_with_keys(
            &mut view,
            &device,
            &mut host,
            &symbols,
            &keys,
            event,
            case,
            payload.clone(),
        );
        eprintln!(
            "[arena] {app} frame {frame} {event}::{case}: {} B outside the scope",
            SYSTEM_BYTES.load(Ordering::Relaxed) - sys0
        );
        let boxes = common::layout_boxes(&view);
        assert!(!boxes.is_empty(), "{app} frame {frame}: layout produced nothing");
        canary(&view, frame, app);
    }
    // A non-structural dispatch (an absorber, a value that changed nothing
    // visible) produces no damage and therefore no emit — so at most every
    // frame opens a generation, and at least every structural one does. Each
    // scenario alternates a structural case with another, so half is the floor.
    let opened = GENERATIONS.load(Ordering::Relaxed) - before;
    if std::env::var_os("ARENA_OFF").is_none() {
        assert!(
            opened >= frames / 2,
            "{app}: {opened} generations for {frames} frames — the scope was not held"
        );
    }
}

/// Text focus survives many emits by binding identity (RFC-0075) — the one
/// long-lived structure `emit()` used to rebuild INSIDE the scope.
fn drive_text_focus(frames: usize) {
    use nexus_dsl_runtime::interact::HandlerAction;
    use nexus_layout_types::FxPx;
    let app = "greeter";
    let nxir = common::compile(app);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(&nxir);
    let keys = common::program_i18n_keys(&nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    if std::env::var_os("ARENA_OFF").is_none() {
        view.set_frame_scope(&EMIT_SCOPE);
    }
    let mut host = NoIo;
    let boxes = common::layout_boxes(&view);
    // The first text-bound field, by its handler.
    let field = view
        .handlers()
        .iter()
        .find(|(_, h)| matches!(h.action, HandlerAction::Bind { .. }))
        .map(|(id, _)| *id)
        .expect("the greeter has a text field");
    let rect = boxes.iter().find(|b| b.node_id == field).expect("field box").rect;
    let (cx, cy) = (rect.x.0 + rect.width.0 / 2, rect.y.0 + rect.height.0 / 2);
    assert!(
        view.focus_text_at(&boxes, FxPx::new(cx), FxPx::new(cy), None).is_some(),
        "focus lands"
    );
    for frame in 0..frames {
        // Each keystroke re-emits (the bound value changed) and revalidates
        // the focus — which must survive the generations that follow.
        let damage = view.insert_text(&tokens, &device, &locale, &mut host, "a").expect("types");
        assert!(damage.is_some(), "frame {frame}: typing must re-emit");
        assert!(view.text_focus().is_some(), "frame {frame}: focus lost across re-emits");
        canary(&view, frame, app);
    }
}

#[test]
fn nothing_emission_allocates_outlives_its_generation() {
    const FRAMES: usize = 64;
    drive(
        "desktop-shell",
        "PanelEvent",
        &[("SetPanel", vec![Value::Str("control".into())]), ("PanelNoop", vec![])],
        FRAMES,
    );
    // `settings` is deliberately absent: driving `WindowEvent::WinMenu` for
    // 34 frames returns `Malformed` from the runtime on a PLAIN heap too
    // (verified with `ARENA_OFF=1`), so it would fail this test for a reason
    // this test is not about. Recorded as a defect in TASK-0077C's ledger.
    drive("stash", "StashEvent", &[("WinLeft", vec![]), ("WinRight", vec![])], FRAMES);
    drive_text_focus(FRAMES);
}
