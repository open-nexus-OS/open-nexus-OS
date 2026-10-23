// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What ONE frame of each real app allocates per arena region (ADR-0065; TASK-0251 step 3b, the
//! 1080p size sweep, finding 8): Emit (the scene the runtime emits) and Layout (app-host's
//! `relayout_in_generation`: boxes + text runs), counted at the window size windowd gives each
//! app on the board's 1920×1080 display and on the lanes' 1280×800, list apps with a full first
//! page of data. On the OS the arena is a bump per region and half: every byte a frame
//! allocates there is spent until the half resets, frees included — so the count is the sum of
//! allocations, never a net.
//!
//! The recorded costs are a budget: growth past them is a red test, not a board surprise. Every
//! budget fits one half of the OS arena (`nexus_service_entry::FRAME_ARENA_HALF`) — the arena is
//! reserved and committed on touch (ADR-0077, TASK-0333 6b-2), so it is sized for the largest
//! frame, the chat's, and a frame that spills on the board is a FAIL, never tolerated. The
//! chat's cost itself is the DSL/layout lane's (pretext completion, TASK-0251 finding 8).
//!
//! One test function, phases measured sequentially (the counters are process-global).

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use nexus_dsl_runtime::{
    EffectHost, FixtureEnv, FrameScope, FrameScopeGuard, IdentityLocale, QueryCall, QueryPage,
    Value, View,
};
use nexus_query::{Engine, MemKv, QType, QVal, QuerySpec, TableDef};

// ------------------------------------------------------------ the counter

const OFF: usize = 0;
const EMIT: usize = 1;
const LAYOUT: usize = 2;

/// The phase allocations are charged to (OFF outside every frame phase).
static PHASE: AtomicUsize = AtomicUsize::new(OFF);
/// Bytes allocated in the CURRENT phase, per phase.
static BYTES: [AtomicUsize; 3] = [AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)];

/// A bump's cost of one allocation: its size rounded up to its alignment (the arena carves
/// aligned blocks back to back; 8 is the floor the allocator hands out).
fn cost(l: Layout) -> usize {
    let align = l.align().max(8);
    l.size().div_ceil(align) * align
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let phase = PHASE.load(Ordering::Relaxed);
        if phase != OFF {
            BYTES[phase].fetch_add(cost(l), Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let phase = PHASE.load(Ordering::Relaxed);
        if phase != OFF {
            BYTES[phase].fetch_add(cost(l), Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // A bump grows by carving the new size and copying: the old block stays spent.
        let phase = PHASE.load(Ordering::Relaxed);
        if phase != OFF {
            let grown = Layout::from_size_align(new, l.align()).unwrap_or(l);
            BYTES[phase].fetch_add(cost(grown), Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, new) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// The largest single frame seen per phase.
static PEAK: [AtomicUsize; 3] = [AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)];

fn begin(phase: usize) {
    BYTES[phase].store(0, Ordering::Relaxed);
    PHASE.store(phase, Ordering::Relaxed);
}

fn end(phase: usize) -> usize {
    PHASE.store(OFF, Ordering::Relaxed);
    let used = BYTES[phase].load(Ordering::Relaxed);
    PEAK[phase].fetch_max(used, Ordering::Relaxed);
    used
}

struct EmitScope;
static EMIT_SCOPE: EmitScope = EmitScope;

impl FrameScope for EmitScope {
    fn enter(&self) -> FrameScopeGuard {
        begin(EMIT);
        // Kept or abandoned, the phase ends: this probe measures, it does not recycle.
        FrameScopeGuard::new(|_completed| {
            let _ = end(EMIT);
        })
    }
}

/// app-host's `relayout_in_generation`: the layout at the surface size, then the text runs
/// (`collect_texts` clones every run's content; modelled by the scene's texts).
fn layout_frame(view: &View, w: i32, h: i32) -> usize {
    begin(LAYOUT);
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine.layout_with_viewport(
        view.scene(),
        nexus_layout_types::FxPx::new(w),
        Some(nexus_layout_types::FxPx::new(h)),
        &nexus_text_baked::measure_text::BakedTextMeasure,
    );
    let texts = common::scene_texts(view);
    let used = end(LAYOUT);
    drop((layout, texts));
    used
}

// -------------------------------------------------------- the chat's data

/// The chat's message source (the on-device `AppEffectHost::query()` twin, as in `chat.rs`).
struct TranscriptHost {
    engine: Engine,
    kv: MemKv,
    seq_sym: u32,
    text_sym: u32,
}

impl TranscriptHost {
    fn seeded(symbols: &[String], n: i64) -> Self {
        let engine = Engine::new(vec![TableDef {
            id: 0,
            columns: vec![QType::Int, QType::Str],
            pk_col: 0,
            indexed: vec![0],
        }]);
        let mut kv = MemKv::new();
        for seq in 1..=n {
            let text = format!("message {seq}: a line of ordinary length in a chat transcript");
            engine.put(&mut kv, 0, &[QVal::Int(seq), QVal::Str(text)]).expect("seed row");
        }
        let sym = |name: &str| symbols.iter().position(|s| s == name).expect(name) as u32;
        Self { engine, kv, seq_sym: sym("seq"), text_sym: sym("text") }
    }
}

impl EffectHost for TranscriptHost {
    fn call(&mut self, _: &str, _: &str, _: &[Value]) -> Result<Value, u32> {
        Err(u32::MAX)
    }

    fn query(&mut self, call: &QueryCall) -> Result<QueryPage, u32> {
        let spec = QuerySpec {
            table: 0,
            eq: vec![],
            range: None,
            order_col: 0,
            descending: call.descending,
            limit: call.limit,
        };
        // The mount's first page is the frame measured; no continuation is handed out.
        let page = self.engine.query(&self.kv, &spec, None).map_err(|_| 9u32)?;
        let rows = page
            .rows
            .into_iter()
            .map(|row| {
                let mut fields: Vec<(u32, Value)> = row
                    .into_iter()
                    .enumerate()
                    .map(|(i, qv)| {
                        let sym = if i == 0 { self.seq_sym } else { self.text_sym };
                        let v = match qv {
                            QVal::Int(n) => Value::Int(n),
                            QVal::Str(s) => Value::Str(s),
                            QVal::Bool(b) => Value::Bool(b),
                            QVal::Fx(f) => Value::Fx(f),
                        };
                        (sym, v)
                    })
                    .collect();
                fields.sort_by_key(|(s, _)| *s);
                Value::Record(fields)
            })
            .collect();
        Ok(QueryPage { rows: Value::List(rows), next: String::new() })
    }
}

/// A host for apps whose data does not change the frame's size.
struct NoIo;
impl EffectHost for NoIo {
    fn call(&mut self, _: &str, _: &str, _: &[Value]) -> Result<Value, u32> {
        Ok(Value::Bool(true))
    }
}

/// The size classes app-host derives from the surface width (`probe/env.rs::size_class_for`).
fn device_at(w: i32) -> FixtureEnv {
    let mut env = FixtureEnv::desktop();
    env.size_class = if w < 640 {
        "compact"
    } else if w < 1024 {
        "regular"
    } else {
        "wide"
    };
    env
}

/// Mounts `app` at `w`×`h`, runs its mount effects inside the measured scope, lays out one
/// frame: `(emit bytes, layout bytes, boxes, texts)` of that frame, and the mounted view.
/// One measured frame: bytes per region, and what the layout held.
struct Frame {
    emit: usize,
    layout: usize,
    boxes: usize,
    texts: usize,
}

fn measure(app: &str, w: i32, h: i32, host: &mut dyn EffectHost) -> Frame {
    let nxir: &'static [u8] = Box::leak(common::compile(app).into_boxed_slice());
    let symbols = common::program_symbols(nxir);
    let keys = common::program_i18n_keys(nxir);
    let device = device_at(w);
    let tokens = nexus_theme_tokens::BaseTokens;
    let mut view = {
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        View::mount(nxir, &tokens, &device, &locale).expect("mounts")
    };
    view.set_frame_scope(&EMIT_SCOPE);
    PEAK[EMIT].store(0, Ordering::Relaxed);
    {
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let _ = view.run_initial_effects(&tokens, &device, &locale, host);
    }
    let emit = PEAK[EMIT].load(Ordering::Relaxed);
    let layout = layout_frame(&view, w, h);
    let boxes = common::layout_boxes(&view).len();
    let texts = common::scene_texts(&view).len();
    Frame { emit, layout, boxes, texts }
}

/// The recorded cost of one frame, bytes `(emit, layout)` — measured 2026-10-09 plus ~10 %.
/// `settings` emits nothing at mount without its services (the stub host), so only its layout
/// is held.
const BUDGET: [(&str, usize, usize); 4] = [
    // chat re-measured 2026-10-21 (TASK-0337: window-kit's chrome in its own surface — the
    // bar and the window scaffold moved from windowd into the app): 296 368 / 469 544 B.
    ("chat", 326_000, 517_000),
    ("settings", 0, 243_000),
    ("stash", 99_000, 210_000),
    ("desktop-shell", 64_000, 103_000),
];

/// The arena holds every recorded frame — checked when this file builds, so the one test
/// function stays alone with the process-global counters: a budget past one half would be a
/// spill on the board that this file's own numbers predicted.
const _: () = {
    let mut i = 0;
    while i < BUDGET.len() {
        let (_, emit, layout) = BUDGET[i];
        let half = nexus_service_entry::FRAME_ARENA_HALF;
        assert!(emit <= half && layout <= half, "a recorded frame outgrew the arena's half");
        i += 1;
    }
};

#[test]
fn a_frame_stays_within_its_recorded_cost_on_every_display() {
    // (app, the window windowd gives it: 1920×1080 board, 1280×800 lanes, and maximized on the
    // board — the display less the desktop taskbar, the largest frame an app window draws)
    let windows = [
        ("chat", (1440, 814), (960, 620), Some((1920, 1024))),
        ("settings", (1440, 814), (960, 620), Some((1920, 1024))),
        ("stash", (1440, 814), (960, 620), Some((1920, 1024))),
        ("desktop-shell", (1920, 1080), (1280, 800), None),
    ];
    for (app, board, lane, maximized) in windows {
        let (_, max_emit, max_layout) =
            BUDGET.iter().copied().find(|(name, _, _)| *name == app).expect("a budget");
        let sizes =
            [("1920x1080", Some(board)), ("1280x800", Some(lane)), ("maximized", maximized)];
        for (label, (w, h)) in sizes.into_iter().filter_map(|(l, s)| s.map(|s| (l, s))) {
            let symbols0 =
                common::program_symbols(Box::leak(common::compile(app).into_boxed_slice()));
            let mut host: Box<dyn EffectHost> = if app == "chat" {
                Box::new(TranscriptHost::seeded(&symbols0, 240))
            } else {
                Box::new(NoIo)
            };
            let Frame { emit, layout, boxes, texts } = measure(app, w, h, host.as_mut());
            println!(
                "[arena] {app:<14} {label:<9} window {w}x{h}: emit {emit:>7} B  layout {layout:>7} B  boxes {boxes:>4}  texts {texts:>4}"
            );
            assert!(emit <= max_emit, "{app} at {label}: a frame's scene grew to {emit} B");
            assert!(layout <= max_layout, "{app} at {label}: a frame's layout grew to {layout} B");
        }
    }
}
