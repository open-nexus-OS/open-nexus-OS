// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the WM title bar rendered FROM the
//! window WIDGET (windows-as-widgets P3.2): `ui/widgets/window` chrome
//! (`WindowControls` + text title) → `nexus-layout` → `nexus-scene-raster`
//! (the promoted, golden-verified painter SSOT) into ONE shared raster cache;
//! the band blit copies rows out of it. This retires the hand-drawn
//! `draw_title_bar_row` — windowd stops drawing chrome pixels by hand, the
//! widget is the design SSOT. Chrome-cache pattern: rasterize ONCE per chrome
//! state change (width/hover/theme/radius), NEVER per frame/per blit.
//! OWNERS: @ui
//! STATUS: Functional (P3.2)
//! API_STABILITY: Unstable
//! ADR: docs/dev/ui/patterns/windowing/windows-as-widgets.md

use super::*;
use crate::compositor::shell_window::{round_top_corners, TitleButton};
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Spacer, Stack,
    VisualStyle,
};
use nexus_theme_tokens::TypographyToken;
use nexus_widget_window::chrome::WindowControls;

/// Title-bar base tint alpha — the bar is part of the frosted material (a
/// translucent wash over the blurred backdrop), identical to the retired
/// hand-raster's contract.
const TITLE_BAR_TINT_ALPHA: u8 = 150;
/// Hover wash alpha over the hovered button zone.
const HOVER_ALPHA: u8 = 96;
/// Title text left inset (px) — parity with the retired hand-raster.
const TITLE_INSET_X: i32 = 14;
/// Button gap picked so the widget's 30 px buttons center in the `Frame`
/// hit-test SSOT's 40 px zones (zone centers at w−100/−60/−20): right
/// padding 5 + gap 10 ⇒ button centers at w−20/−60/−100. Visual == hit zone.
const CONTROLS_GAP: i32 = 10;
const CONTROLS_PAD_RIGHT: i32 = 5;

/// Widget ids for the three chrome buttons — the hover wash targets the
/// laid-out box by this id (`LayoutBox::id` survives layout).
const ID_MIN: &str = "win_min";
const ID_MAX: &str = "win_max";
const ID_CLOSE: &str = "win_close";

/// ONE shared rasterized title bar, keyed by its full visual state. Windows
/// share the chrome design; re-rasters happen only when a blit needs a
/// different (width, hover, theme, radius) — i.e. on hover/resize/theme
/// changes, never per frame (the bounded band blits skip title rows entirely).
pub(crate) struct ChromeCache {
    pub(crate) buf: alloc::vec::Vec<u8>,
    w: u32,
    title_h: u32,
    hover: Option<TitleButton>,
    radius: u32,
    dark: bool,
    valid: bool,
}

impl ChromeCache {
    pub(crate) fn new() -> Self {
        Self {
            buf: alloc::vec::Vec::new(),
            w: 0,
            title_h: 0,
            hover: None,
            radius: 0,
            dark: false,
            valid: false,
        }
    }
}

/// Static token sets — the widget builds against the SAME theme vocabulary
/// the DSL apps render with (one design SSOT).
static DARK_TOKENS: nexus_theme_tokens::DarkTokens = nexus_theme_tokens::DarkTokens;
static LIGHT_TOKENS: nexus_theme_tokens::LightTokens = nexus_theme_tokens::LightTokens;

impl DisplayServerRuntime {
    /// Ensure the shared chrome raster matches `(w, title_h, hover, radius,
    /// theme)`; rebuilds (widget → layout → scene-raster + glyph pass +
    /// corner mask) only on a key change. The cache buffer is grow-only and
    /// reused (non-freeing heap discipline).
    pub(super) fn ensure_chrome_cache(
        &mut self,
        w: u32,
        title_h: u32,
        hover: Option<TitleButton>,
        radius: u32,
    ) {
        let dark = matches!(self.theme_mode, crate::theme::ThemeMode::Dark);
        {
            let c = &self.chrome_cache;
            if c.valid
                && c.w == w
                && c.title_h == title_h
                && c.hover == hover
                && c.radius == radius
                && c.dark == dark
            {
                return;
            }
        }
        let sa = self.theme().surface_alt;
        let tokens: &'static dyn nexus_theme_tokens::Tokens =
            if dark { &DARK_TOKENS } else { &LIGHT_TOKENS };

        // 1. The title bar as a WIDGET subtree: [title text · spacer · controls].
        let title = nexus_widget_text::Text::new("App")
            .size(TypographyToken::Sm)
            .build(tokens);
        let controls = WindowControls::new()
            .minimize(ID_MIN)
            .maximize(ID_MAX)
            .close(ID_CLOSE)
            .gap(CONTROLS_GAP)
            .build(tokens);
        let bar = LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets {
                    left: FxPx::new(TITLE_INSET_X),
                    right: FxPx::new(CONTROLS_PAD_RIGHT),
                    top: FxPx::ZERO,
                    bottom: FxPx::ZERO,
                },
                align: Align::Center,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: Some(FxPx::new(w as i32)),
                max_width: Some(FxPx::new(w as i32)),
                min_height: Some(FxPx::new(title_h as i32)),
                max_height: Some(FxPx::new(title_h as i32)),
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            alloc::vec![
                title,
                LayoutNode::Spacer(Spacer {
                    id: None,
                    flex_grow: 1,
                    min_size: None,
                    item: FlexItem::default(),
                }),
                controls,
            ],
        );

        // 2. Layout at exactly the bar size.
        let engine = nexus_layout::LayoutEngine::new();
        let Ok(layout) = engine.layout_with_viewport(
            &bar,
            FxPx::new(w as i32),
            Some(FxPx::new(title_h as i32)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        ) else {
            return; // keep the previous raster; next change retries
        };

        // 3. Hover wash on the hovered button's laid-out box (id → node_id).
        let hover_wash = hover.and_then(|b| {
            let id = match b {
                TitleButton::Minimize => ID_MIN,
                TitleButton::Maximize => ID_MAX,
                TitleButton::Close => ID_CLOSE,
            };
            layout.boxes.iter().find(|bx| bx.id == Some(id)).map(|bx| {
                let a = tokens.color(nexus_theme_tokens::ColorToken::Accent);
                nexus_scene_raster::HoverWash {
                    node_id: bx.node_id,
                    color: nexus_layout_types::Rgba8::new(a.r, a.g, a.b, HOVER_ALPHA),
                }
            })
        });

        // 4. Rasterize: translucent material base + widget boxes + glyphs +
        //    the rounded-top-corner mask (same contract as the hand version).
        let base = [sa[0], sa[1], sa[2], TITLE_BAR_TINT_ALPHA]; // BGRA
        let c = &mut self.chrome_cache;
        let row_bytes = w as usize * 4;
        c.buf.resize(row_bytes * title_h as usize, 0);
        for y in 0..title_h as i32 {
            let row = &mut c.buf[y as usize * row_bytes..(y as usize + 1) * row_bytes];
            for px in row.chunks_exact_mut(4) {
                px.copy_from_slice(&base);
            }
            {
                let mut canvas = nexus_scene_raster::RowCanvas::new(row, y, w as i32);
                nexus_scene_raster::paint_row_hover(&mut canvas, &layout.boxes, hover_wash);
            }
            // Glyph pass: the title Text node(s).
            for (node_id, content, font, color) in chrome_texts(&bar) {
                if let Some(bx) = layout.boxes.iter().find(|b| b.node_id == node_id) {
                    let (tx, ty, th) =
                        (bx.rect.x.0, bx.rect.y.0, bx.rect.height.0);
                    if y >= ty && y < ty + th {
                        nexus_text_baked::draw_text_row(
                            row,
                            y as u32,
                            ty,
                            tx.max(0) as u32,
                            w,
                            content.chars(),
                            font,
                            color,
                        );
                    }
                }
            }
            round_top_corners(y as u32, row, w, radius);
        }
        c.w = w;
        c.title_h = title_h;
        c.hover = hover;
        c.radius = radius;
        c.dark = dark;
        c.valid = true;
    }
}

/// Pre-order text runs of the chrome subtree: `(node_id, content, font,
/// BGRA color)` — the same numbering contract the layout engine stamps
/// (`LayoutBox::node_id`), so the glyph pass can position each run.
fn chrome_texts(
    node: &LayoutNode,
) -> alloc::vec::Vec<(usize, alloc::string::String, nexus_text_baked::FontSize, [u8; 4])> {
    let mut out = alloc::vec::Vec::new();
    let mut index = 0usize;
    collect(node, &mut index, &mut out);
    return out;

    fn collect(
        node: &LayoutNode,
        index: &mut usize,
        out: &mut alloc::vec::Vec<(usize, alloc::string::String, nexus_text_baked::FontSize, [u8; 4])>,
    ) {
        use nexus_layout_types::LayoutNode as N;
        *index += 1;
        match node {
            N::Text(text, _) => {
                let font = if text.style.font_size.0 >= 15 {
                    nexus_text_baked::FontSize::Body
                } else {
                    nexus_text_baked::FontSize::Small
                };
                let c = text.style.color;
                out.push((
                    *index,
                    alloc::string::String::from(text.content.as_str()),
                    font,
                    [c.b, c.g, c.r, c.a],
                ));
            }
            N::Stack(_, _, children) | N::Grid(_, _, children) => {
                for child in children {
                    collect(child, index, out);
                }
            }
            _ => {}
        }
    }
}
