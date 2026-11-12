// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What soft float costs the two CPU rasters (TASK-0362 P0): the row painter
//! (`nexus-scene-raster`, the app surfaces) and the SVG raster (`nexus-svg`, the icons and the
//! compute broker's job), the crates of the image unchanged, timed on the stock system.
//!
//! A static program without a C library: it speaks the stock kernel's syscalls directly, so it
//! needs nothing installed on the board. `build.sh` builds it three ways with the repo's pinned
//! toolchain — `soft` (the image's target today), `hard` (F and D), `hardv` (F, D and the vector
//! extension, the compiler's own vectorisation: no hand-written kernel) — and the three programs
//! print the same lines.
//!
//! One line per workload:
//! `rasterbench build=hard scene=shadow px=460800 min_ns=… ns_per_px=… first_ns=… digest=…`
//! `min_ns` is the fastest of five runs after one warm-up, `first_ns` the warm-up (cold caches,
//! and the first use of a register set the stock kernel enables on demand), `ns_per_px` is
//! `min_ns` over the pixels of the target (rows × width, painted or not — the measure of
//! `apphost: hops motion paint_us`), `digest` the FNV-1a of the pixels: equal across the three
//! builds, or the build changed the picture. An SVG workload adds
//! `rasterbench build=… scene=… stages parse_ns=… plan_ns=… raster_ns=…` (its fastest run), and
//! the vector build starts with `rasterbench build=hardv vlenb=…` (the registers' width in bytes).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::arch::{asm, global_asm};
use core::cell::{Cell, UnsafeCell};
use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use nexus_layout::LayoutBox;
use nexus_layout_types::{
    BoxShadow, CornerRadius, EdgeBorder, FxPx, PathPoint, PathShape, Rect, Rgba8, ShapeKind,
    VisualStyle,
};
use nexus_scene_raster::{paint_row, RowCanvas};

global_asm!(
    ".section .text._start,\"ax\",@progbits",
    ".globl _start",
    "_start:",
    "andi sp, sp, -16",
    "call {main}",
    main = sym bench_main,
);

// ——— the stock kernel ———

const SYS_WRITE: usize = 64;
const SYS_EXIT_GROUP: usize = 94;
const SYS_CLOCK_GETTIME: usize = 113;
const SYS_SCHED_SETAFFINITY: usize = 122;
const CLOCK_MONOTONIC: usize = 1;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

fn syscall3(nr: usize, a0: usize, a1: usize, a2: usize) -> isize {
    let ret: isize;
    // SAFETY: a Linux system call; the arguments are plain values or pointers to live memory.
    unsafe {
        asm!("ecall", in("a7") nr, inlateout("a0") a0 => ret, in("a1") a1, in("a2") a2);
    }
    ret
}

fn now_ns() -> u64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    syscall3(SYS_CLOCK_GETTIME, CLOCK_MONOTONIC, &mut ts as *mut Timespec as usize, 0);
    ts.sec as u64 * 1_000_000_000 + ts.nsec as u64
}

fn exit(code: usize) -> ! {
    loop {
        syscall3(SYS_EXIT_GROUP, code, 0, 0);
    }
}

struct Out;

impl Write for Out {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let mut rest = s.as_bytes();
        while !rest.is_empty() {
            let n = syscall3(SYS_WRITE, 1, rest.as_ptr() as usize, rest.len());
            if n <= 0 {
                return Err(core::fmt::Error);
            }
            rest = &rest[n as usize..];
        }
        Ok(())
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    let _ = writeln!(Out, "rasterbench: panic {info}");
    exit(101)
}

// ——— memory: one arena, handed back after every run ———

const HEAP_BYTES: usize = 96 << 20;

struct Arena {
    mem: UnsafeCell<[u8; HEAP_BYTES]>,
    top: AtomicUsize,
}

// SAFETY: the program has one thread.
unsafe impl Sync for Arena {}

// SAFETY: every block lies inside `mem`, aligned as asked, and no two live blocks overlap (the
// top only moves up between two `rewind`s).
unsafe impl GlobalAlloc for Arena {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let base = self.mem.get() as usize;
        let start = (base + self.top.load(Ordering::Relaxed) + layout.align() - 1)
            & !(layout.align() - 1);
        let end = start + layout.size();
        if end > base + HEAP_BYTES {
            return core::ptr::null_mut();
        }
        self.top.store(end - base, Ordering::Relaxed);
        start as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ARENA: Arena =
    Arena { mem: UnsafeCell::new([0; HEAP_BYTES]), top: AtomicUsize::new(0) };

/// Runs `f` and hands its memory back. `R` is plain numbers: nothing `f` allocated outlives it.
fn in_arena<R: Copy>(f: impl FnOnce() -> R) -> R {
    let mark = ARENA.top.load(Ordering::Relaxed);
    let r = f();
    ARENA.top.store(mark, Ordering::Relaxed);
    r
}

// ——— measuring ———

const RUNS: usize = 5;

fn build_name() -> &'static str {
    if cfg!(target_feature = "v") {
        "hardv"
    } else if cfg!(target_feature = "d") {
        "hard"
    } else {
        "soft"
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// One warm-up and `RUNS` timed runs of `run` (which returns its pixels' digest).
fn measure(scene: &str, px: u64, mut run: impl FnMut() -> (u64, u64)) {
    let (first_ns, digest) = run();
    let mut min_ns = u64::MAX;
    for _ in 0..RUNS {
        let (ns, d) = run();
        if d != digest {
            let _ = writeln!(Out, "rasterbench: scene={scene} is not deterministic");
            exit(2);
        }
        min_ns = min_ns.min(ns);
    }
    let tenths = min_ns * 10 / px.max(1);
    let _ = writeln!(
        Out,
        "rasterbench build={} scene={scene} px={px} min_ns={min_ns} ns_per_px={}.{} \
         first_ns={first_ns} digest={digest:016x}",
        build_name(),
        tenths / 10,
        tenths % 10,
    );
}

// ——— the row painter ———

/// A full-HD surface's width — the width `apphost: hops motion` reports its rows at.
const WIDTH: i32 = 1920;
const ROWS: i32 = 240;

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Rgba8 {
    Rgba8 { r, g, b, a }
}

fn boxed(x: i32, y: i32, w: i32, h: i32, visual: VisualStyle) -> LayoutBox {
    LayoutBox {
        node_id: 1,
        rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
        visual,
        ..LayoutBox::default()
    }
}

/// The window's own fill: one opaque box over every pixel.
fn ground() -> LayoutBox {
    boxed(
        0,
        0,
        WIDTH,
        ROWS,
        VisualStyle { background: Some(rgba(28, 28, 32, 255)), ..VisualStyle::default() },
    )
}

/// Six translucent rounded cards with a hairline and a top shine (a settings page's rows).
fn cards(shadow: Option<BoxShadow>) -> Vec<LayoutBox> {
    (0..6)
        .map(|i| {
            boxed(
                40 + i * 310,
                24,
                280,
                180,
                VisualStyle {
                    background: Some(if shadow.is_some() {
                        rgba(52, 52, 60, 255)
                    } else {
                        rgba(255, 255, 255, 36)
                    }),
                    corner_radius: CornerRadius::uniform(FxPx::new(16)),
                    border: EdgeBorder::all(FxPx::new(1), rgba(255, 255, 255, 48)),
                    inset_highlight: Some(rgba(255, 255, 255, 28)),
                    shadow,
                    ..VisualStyle::default()
                },
            )
        })
        .collect()
}

/// Twelve round buttons.
fn circles() -> Vec<LayoutBox> {
    (0..12)
        .map(|i| {
            boxed(
                30 + i * 156,
                60,
                120,
                120,
                VisualStyle {
                    background: Some(rgba(64, 132, 255, 230)),
                    shape: ShapeKind::Circle,
                    ..VisualStyle::default()
                },
            )
        })
        .collect()
}

/// A line icon of the imported set's kind: three polylines in the 0..1000 box, stroked 2/24 wide.
fn line_icon() -> ShapeKind {
    let p = PathPoint::new;
    ShapeKind::Stroke {
        paths: vec![
            PathShape::polygon(&[p(125, 208), p(875, 208), p(875, 792), p(125, 792)]),
            PathShape::line(&[p(125, 375), p(500, 625), p(875, 375)]),
            PathShape::line(&[p(292, 792), p(292, 917), p(708, 917), p(708, 792)]),
        ],
        width_milli: 83,
    }
}

/// A filled symbol: a ten-point star and a diamond cut out of it by the even-odd rule.
fn filled_icon() -> ShapeKind {
    let p = PathPoint::new;
    ShapeKind::Vector(vec![
        PathShape::polygon(&[
            p(500, 20),
            p(618, 345),
            p(976, 345),
            p(690, 560),
            p(794, 900),
            p(500, 700),
            p(206, 900),
            p(310, 560),
            p(24, 345),
            p(382, 345),
        ]),
        PathShape::polygon(&[p(500, 380), p(590, 480), p(500, 580), p(410, 480)]),
    ])
}

fn icons(shape: fn() -> ShapeKind) -> Vec<LayoutBox> {
    (0..24)
        .map(|i| {
            boxed(
                24 + i * 78,
                88,
                64,
                64,
                VisualStyle {
                    background: Some(rgba(240, 240, 245, 255)),
                    shape: shape(),
                    ..VisualStyle::default()
                },
            )
        })
        .collect()
}

fn with_ground(mut over: Vec<LayoutBox>) -> Vec<LayoutBox> {
    over.insert(0, ground());
    over
}

fn soft_shadow() -> BoxShadow {
    BoxShadow {
        offset_x: FxPx::new(0),
        offset_y: FxPx::new(8),
        blur_radius: FxPx::new(24),
        spread: FxPx::new(0),
        color: rgba(0, 0, 0, 120),
    }
}

/// A page as an app paints it: a gradient under shadowed cards, each carrying icons.
fn page() -> Vec<LayoutBox> {
    let mut boxes = vec![boxed(
        0,
        0,
        WIDTH,
        ROWS,
        VisualStyle {
            background_gradient: Some((rgba(34, 36, 48, 255), rgba(18, 18, 22, 255))),
            background: Some(rgba(28, 28, 32, 255)),
            ..VisualStyle::default()
        },
    )];
    boxes.extend(cards(Some(soft_shadow())));
    boxes.extend(icons(line_icon).into_iter().step_by(2));
    boxes.extend(icons(filled_icon).into_iter().skip(1).step_by(2));
    boxes
}

fn paint_scene(scene: &str, boxes: &[LayoutBox], surface: &mut [u8]) {
    let stride = WIDTH as usize * 4;
    measure(scene, (WIDTH * ROWS) as u64, || {
        surface.fill(0);
        let t0 = now_ns();
        for y in 0..ROWS {
            let row = &mut surface[y as usize * stride..(y as usize + 1) * stride];
            let mut canvas = RowCanvas::new(row, y, WIDTH);
            paint_row(&mut canvas, boxes);
        }
        let ns = now_ns() - t0;
        (ns, fnv1a(surface))
    });
}

// ——— the SVG raster ———

const LOGO: &str = include_str!("../../../../../../resources/icons/logos/open-nexus.svg");
const APP_ICON: &str = include_str!("../../../../../../resources/app-icons/productivity/recipes.svg");

/// Parse, plan and rasterise `src` at `size` × `size` — what one icon costs from its text. A
/// second line names the three stages of the fastest run.
fn svg_scene(scene: &str, src: &str, size: u32) {
    let fastest = Cell::new((u64::MAX, 0u64, 0u64, 0u64));
    measure(scene, u64::from(size) * u64::from(size), || {
        in_arena(|| {
            let t0 = now_ns();
            let mut marks = (t0, t0);
            let rendered = nexus_svg::parse_svg(src).and_then(|doc| {
                marks.0 = now_ns();
                let plan = nexus_svg::plan_document_at(&doc, size, size)?;
                marks.1 = now_ns();
                let mut scratch = plan.scratch();
                let mut out = vec![0u8; (size * size) as usize * nexus_svg::OUTPUT_BYTES_PER_PIXEL];
                plan.rasterize_rows(0, size, &mut scratch, &mut out)?;
                Ok(out)
            });
            let t3 = now_ns();
            let ns = t3 - t0;
            if ns < fastest.get().0 {
                fastest.set((ns, marks.0 - t0, marks.1 - marks.0, t3 - marks.1));
            }
            match rendered {
                Ok(out) => (ns, fnv1a(&out)),
                Err(e) => {
                    let _ = writeln!(Out, "rasterbench: scene={scene} failed: {e:?}");
                    exit(3)
                }
            }
        })
    });
    let (_, parse_ns, plan_ns, raster_ns) = fastest.get();
    let _ = writeln!(
        Out,
        "rasterbench build={} scene={scene} stages parse_ns={parse_ns} plan_ns={plan_ns} \
         raster_ns={raster_ns}",
        build_name(),
    );
}

/// The vector registers' width in bytes, as the hart states it (`vlenb`).
#[cfg(target_feature = "v")]
fn vector_bytes() -> usize {
    let bytes: usize;
    // SAFETY: reads a read-only register of the vector extension this build requires.
    unsafe {
        asm!("csrr {0}, vlenb", out(reg) bytes);
    }
    bytes
}

extern "C" fn bench_main() -> ! {
    // One CPU for the whole run: the numbers of two builds are compared, not two harts.
    let mask: u64 = 1;
    syscall3(SYS_SCHED_SETAFFINITY, 0, 8, &mask as *const u64 as usize);

    #[cfg(target_feature = "v")]
    let _ = writeln!(Out, "rasterbench build={} vlenb={}", build_name(), vector_bytes());

    let mut surface = vec![0u8; (WIDTH * ROWS) as usize * 4];
    paint_scene("fill", &[ground()], &mut surface);
    paint_scene("cards", &with_ground(cards(None)), &mut surface);
    paint_scene("shadow", &with_ground(cards(Some(soft_shadow()))), &mut surface);
    paint_scene("circles", &with_ground(circles()), &mut surface);
    paint_scene("strokes", &with_ground(icons(line_icon)), &mut surface);
    paint_scene("filled", &with_ground(icons(filled_icon)), &mut surface);
    paint_scene("page", &page(), &mut surface);

    svg_scene("svg-logo-256", LOGO, 256);
    svg_scene("svg-logo-1024", LOGO, 1024);
    svg_scene("svg-icon-64", APP_ICON, 64);
    svg_scene("svg-icon-512", APP_ICON, 512);
    exit(0)
}
