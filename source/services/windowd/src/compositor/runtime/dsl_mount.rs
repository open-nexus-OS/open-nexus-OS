// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the DSL demo window (TASK-0076B):
//! the first *visible* in-compositor mount of a compiled `.nxir` program.
//! A fourth `ShellWindow` whose body is rendered from the DSL interpreter's
//! retained scene (`nexus-dsl-runtime::View` → `LayoutEngine` → per-row box
//! walk), with live pointer taps routed through the interpreter's hit-testing.
//! Mirrors the Settings window's atlas lifecycle; auto-opens once at boot so
//! the QEMU window shows a DSL-rendered page without interaction.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: interpreter behavior host-tested (tests/dsl_goldens); the
//! mount itself is proven via QEMU markers (`DSL: …`).

use super::*;
use nexus_dsl_runtime::theme_tokens::BaseTokens;
use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale, NoIo, View};
use nexus_layout::{LayoutEngine, LayoutResult};
use nexus_layout_types::{
    FxPx, LayoutNode, LineLayout, LineMetrics, MeasureText, PreparedTextHandle, TextContent,
    TextStyle,
};

/// The compiled DSL demo program (built by `build.rs` from
/// `examples/dsl/counter/counter.nx` — canonical, hash-verified on mount).
///
/// 8-byte aligned: Cap'n Proto segments are word-aligned by contract, and
/// `include_bytes!` alone guarantees no alignment (misaligned u64 reads are
/// a fault/emulation hazard on riscv).
#[repr(C, align(8))]
struct AlignedNxir<const N: usize>([u8; N]);
static DSL_DEMO_ALIGNED: AlignedNxir<{ include_bytes!(concat!(env!("OUT_DIR"), "/dsl_demo.nxir")).len() }> =
    AlignedNxir(*include_bytes!(concat!(env!("OUT_DIR"), "/dsl_demo.nxir")));
static DSL_DEMO_NXIR: &[u8] = &DSL_DEMO_ALIGNED.0;

pub(crate) const DSL_WIN_W: u32 = 300;
pub(crate) const DSL_WIN_H: u32 = 220;
pub(crate) const DSL_TITLE_H: u32 = 32;
pub(crate) const DSL_CLOSE_W: u32 = 40;
pub(crate) const DSL_RADIUS: u32 = 12;

/// Pixel-accurate text measurement over windowd's baked glyph tables — the
/// live-path `MeasureText` for the DSL page layout. Packs (line height,
/// width) into the opaque handle like the estimate measurer, but with real
/// advances from `text::measure`.
pub(crate) struct BakedTextMeasure;

impl BakedTextMeasure {
    fn font(style: &TextStyle) -> crate::text::FontSize {
        if style.font_size.0 >= 15 {
            crate::text::FontSize::Body
        } else {
            crate::text::FontSize::Small
        }
    }
}

impl MeasureText for BakedTextMeasure {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle {
        let font = Self::font(style);
        let width = crate::text::measure(content.as_str().chars(), font) as usize;
        let line_height = crate::text::line_height(font) as usize;
        PreparedTextHandle((line_height << 20) | (width & 0xF_FFFF))
    }

    fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx {
        FxPx::new((handle.0 & 0xF_FFFF) as i32)
    }

    fn layout_lines(
        &self,
        handle: &PreparedTextHandle,
        width: FxPx,
        max_lines: Option<u32>,
    ) -> LineLayout {
        let natural_width = self.measure_width(handle);
        let line_height = FxPx::new((handle.0 >> 20) as i32);
        let line = LineMetrics {
            text_range: 0..1,
            width: natural_width.min(width.max(FxPx::ONE)),
            baseline: line_height,
            height: line_height,
        };
        let lines = if matches!(max_lines, Some(0)) { alloc::vec![] } else { alloc::vec![line] };
        LineLayout { lines, natural_width }
    }
}

/// Mounted interpreter state for the DSL demo window.
pub(crate) struct DslMount {
    pub view: Option<View<'static>>,
    pub layout: Option<LayoutResult>,
    symbols: alloc::vec::Vec<alloc::string::String>,
    keys: alloc::vec::Vec<u32>,
    pub boot_open_done: bool,
    interaction_marked: bool,
    first_frame_marked: bool,
}

impl DslMount {
    pub(crate) fn new() -> Self {
        Self {
            view: None,
            layout: None,
            symbols: alloc::vec::Vec::new(),
            keys: alloc::vec::Vec::new(),
            boot_open_done: false,
            interaction_marked: false,
            first_frame_marked: false,
        }
    }
}

impl DisplayServerRuntime {
    /// Boot mount (one-shot): called from the present-visible milestone —
    /// the desktop is composited, so the on-demand window pool is live.
    /// (The first scene builds run BEFORE the pool has room, and the
    /// compositor is reactive — a per-frame retry never fires without
    /// damage. Both were observed the hard way; see the task ledger.)
    pub(super) fn maybe_boot_open_dsl(&mut self) {
        if self.dsl_mount.boot_open_done {
            return;
        }
        self.dsl_mount.boot_open_done = true;
        self.open_dsl_demo();
    }

    /// Show the DSL demo window (mirrors `open_settings`): mount the
    /// interpreter (once), acquire atlas surfaces, damage the region.
    pub(super) fn open_dsl_demo(&mut self) {
        if self.shell_config.locked {
            return;
        }
        if !self.ensure_dsl_view() {
            return; // program failed to mount — marker already emitted
        }
        if !self.dsl_win.is_mounted() {
            let w = self.dsl_win.w;
            let h = self.dsl_win.h;
            let Some(content) = self.atlas_alloc.alloc(w, h) else {
                // Hop diagnostics: report the exact budget so a failure here
                // is explained by values, not guesses.
                let _ = debug_println(&alloc::format!(
                    "windowd: dsl open FAIL atlas (need={}x{} rows_remaining={})",
                    w,
                    h,
                    self.atlas_alloc.rows_remaining()
                ));
                return;
            };
            let blur = self.atlas_alloc.alloc(w, h); // best-effort (unblurred without)
            self.dsl_win.mount(content, blur);
        }
        self.dsl_win.visible = true;
        self.show_window(crate::window_scene::WindowId::DslDemo);
        self.dsl_win.surface_dirty = true;
        self.queue_dirty_rect(self.dsl_window_rect());
    }

    pub(super) fn close_dsl_demo(&mut self) {
        self.dsl_win.visible = false;
        self.hide_window(crate::window_scene::WindowId::DslDemo);
        self.dsl_win.end_drag();
        let rect = self.dsl_window_rect();
        if let Some((content, blur)) = self.dsl_win.unmount() {
            self.atlas_alloc.free(content);
            if let Some(blur) = blur {
                self.atlas_alloc.free(blur);
            }
        }
        self.queue_dirty_rect(rect);
    }

    /// Mounts the embedded `.nxir` into the interpreter (once) and lays out
    /// its scene for the window body. Fail-closed: a validation error keeps
    /// the window closed and reports the reason once.
    fn ensure_dsl_view(&mut self) -> bool {
        if self.dsl_mount.view.is_some() {
            return true;
        }
        let symbols = match nexus_dsl_runtime::Runtime::mount(DSL_DEMO_NXIR) {
            Ok(runtime) => runtime.symbols().to_vec(),
            Err(_) => {
                let _ = debug_println("DSL: program mount FAILED (validation)");
                return false;
            }
        };
        let keys: alloc::vec::Vec<u32> =
            match nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(DSL_DEMO_NXIR)
                .and_then(|r| r.root().map(|root| root.get_i18n_keys().map(|l| l.iter().map(|k| k.get_key()).collect())))
            {
                Ok(Ok(keys)) => keys,
                _ => alloc::vec::Vec::new(),
            };
        let view = {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            match View::mount(DSL_DEMO_NXIR, &BaseTokens, &FixtureEnv::default(), &locale)
            {
                Ok(view) => view,
                Err(_) => {
                    let _ = debug_println("DSL: program mount FAILED (emit)");
                    return false;
                }
            }
        };
        // Marker: program loaded — print the leading 8 hash bytes.
        {
            let mut line = alloc::string::String::from("DSL: program loaded hash=");
            if let Ok(root) =
                nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(DSL_DEMO_NXIR)
                    .and_then(|r| {
                        r.root().map(|root| {
                            root.get_program_hash().map(|h| h.to_vec()).unwrap_or_default()
                        })
                    })
            {
                for b in root.iter().take(8) {
                    let hi = b >> 4;
                    let lo = b & 0xf;
                    line.push(char::from_digit(u32::from(hi), 16).unwrap_or('0'));
                    line.push(char::from_digit(u32::from(lo), 16).unwrap_or('0'));
                }
            }
            let _ = debug_println(&line);
        }
        self.dsl_mount.symbols = symbols;
        self.dsl_mount.keys = keys;
        self.dsl_mount.view = Some(view);
        self.relayout_dsl();
        true
    }

    /// Recomputes the body layout from the interpreter's retained scene.
    pub(super) fn relayout_dsl(&mut self) {
        let Some(view) = &self.dsl_mount.view else { return };
        let body_w = self.dsl_win.w.max(1) as i32;
        let engine = LayoutEngine::new();
        self.dsl_mount.layout =
            engine.layout(view.scene(), FxPx::new(body_w), &BakedTextMeasure).ok();
    }

    /// Routes a body click into the interpreter (window-local coordinates,
    /// body space = below the title bar). Damage drives re-render/re-layout.
    pub(super) fn dsl_pointer_body(&mut self, local_x: i32, local_y: i32) {
        let Some(view) = self.dsl_mount.view.as_mut() else { return };
        let Some(layout) = &self.dsl_mount.layout else { return };
        let locale =
            IdentityLocale { symbols: &self.dsl_mount.symbols, keys: &self.dsl_mount.keys };
        let outcome = view.pointer(
            &BaseTokens,
            &FixtureEnv::default(),
            &locale,
            &mut NoIo,
            &layout.boxes,
            "Tap",
            FxPx::new(local_x),
            FxPx::new(local_y - DSL_TITLE_H as i32),
        );
        match outcome {
            Ok(Some(damage)) => {
                if damage == Damage::Layout {
                    self.relayout_dsl();
                }
                if damage != Damage::None {
                    self.dsl_win.surface_dirty = true;
                    self.queue_dirty_rect(self.dsl_window_rect());
                    if !self.dsl_mount.interaction_marked {
                        self.dsl_mount.interaction_marked = true;
                        let _ = debug_println("DSL: interaction visible ok");
                    }
                }
            }
            Ok(None) => {}
            Err(_) => {
                let _ = debug_println("windowd: dsl pointer dispatch error");
            }
        }
    }

    /// Renders the DSL window into its atlas surface: shared title-bar chrome
    /// + glass body + the interpreter scene's layout boxes (fills + text).
    pub(super) fn render_dsl_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let Some(surface) = self.dsl_win.atlas else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let abs_row = surface.abs_row;
        let col_off = surface.x as usize * 4;
        let h = self.dsl_win.h.min(surface.height);
        let w = self.dsl_win.w.min(surface.width);
        let row_bytes = w as usize * 4;
        let title_hover = self.dsl_win.title_hover;
        let corner_radius =
            if self.windows.is_fullscreen(crate::window_scene::WindowId::DslDemo) {
                0
            } else {
                DSL_RADIUS
            };
        let tk = self.theme();
        // Collect the text runs once (node_id → content/style), matched to
        // boxes by pre-order id during the row walk.
        let mut texts: alloc::vec::Vec<(usize, alloc::string::String, crate::text::FontSize, [u8; 4])> =
            alloc::vec::Vec::new();
        if let Some(view) = &self.dsl_mount.view {
            collect_texts(view.scene(), &mut 0, &mut texts);
        }
        let layout = self.dsl_mount.layout.as_ref();
        let band = &mut self.band_scratch;
        for ly in 0..h {
            let row = &mut band[0..stride];
            row[..row_bytes].fill(0);
            if ly < DSL_TITLE_H {
                crate::compositor::shell_window::draw_title_bar_row(
                    ly,
                    row,
                    w,
                    "DSL Demo",
                    DSL_TITLE_H,
                    DSL_CLOSE_W,
                    title_hover,
                    corner_radius,
                    tk,
                )?;
            } else {
                // Glass body tint (same frosted recipe as Settings).
                crate::compositor::desktop_layer::write_tint_span(
                    row,
                    0,
                    w,
                    crate::theme::with_alpha(tk.glass_tint, crate::compositor::desktop_layer::TINT[3]),
                );
                if let Some(layout) = layout {
                    draw_dsl_body_row(row, ly, w, layout, &texts);
                }
            }
            let dst = (abs_row + ly) as usize * stride + col_off;
            vmo_write(handle, dst, &row[..row_bytes])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        if !self.dsl_mount.first_frame_marked {
            self.dsl_mount.first_frame_marked = true;
            let _ = debug_println("DSL: first frame presented");
        }
        Ok(())
    }

    pub(super) fn dsl_window_rect(&self) -> DamageRect {
        self.dsl_win.damage_rect(self.mode.width, self.mode.height)
    }
}

/// Pre-order text collection (index parallels `LayoutBox::node_id` - 1).
fn collect_texts(
    node: &LayoutNode,
    index: &mut usize,
    out: &mut alloc::vec::Vec<(usize, alloc::string::String, crate::text::FontSize, [u8; 4])>,
) {
    *index += 1;
    match node {
        LayoutNode::Text(text, _) => {
            let font = if text.style.font_size.0 >= 15 {
                crate::text::FontSize::Body
            } else {
                crate::text::FontSize::Small
            };
            let c = text.style.color;
            out.push((*index, alloc::string::String::from(text.content.as_str()), font, [c.b, c.g, c.r, c.a]));
        }
        LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => {
            for child in children {
                collect_texts(child, index, out);
            }
        }
        _ => {}
    }
}

/// Draws one body row: box background fills, then text glyph rows.
fn draw_dsl_body_row(
    row: &mut [u8],
    ly: u32,
    w: u32,
    layout: &LayoutResult,
    texts: &[(usize, alloc::string::String, crate::text::FontSize, [u8; 4])],
) {
    let content_y = ly as i32 - DSL_TITLE_H as i32;
    for b in &layout.boxes {
        let (bx, by, bw, bh) =
            (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
        if bw <= 0 || bh <= 0 || content_y < by || content_y >= by + bh {
            continue;
        }
        if let Some(bg) = b.visual.background {
            let x0 = bx.max(0) as u32;
            let x1 = ((bx + bw).max(0) as u32).min(w);
            if x1 > x0 {
                crate::compositor::desktop_layer::write_tint_span(
                    row,
                    x0,
                    x1,
                    [bg.b, bg.g, bg.r, bg.a],
                );
            }
        }
        if let Some((_, content, font, color)) =
            texts.iter().find(|(id, _, _, _)| *id == b.node_id)
        {
            crate::text::draw_text_row(
                row,
                ly,
                by + DSL_TITLE_H as i32,
                bx.max(0) as u32,
                w,
                content.chars(),
                *font,
                *color,
            );
        }
    }
}
