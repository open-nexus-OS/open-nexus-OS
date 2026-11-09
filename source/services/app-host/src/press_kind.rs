// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What a PRESS does to a control (RFC-0116 — the design artifacts' press feedback):
//! held while the pointer is down (windowd's TAP is the press, its RELEASE the release) and let go
//! with the release. The category follows the shape the v3 rules give a role: an action control
//! is a capsule or a circle and dips to 95 % (`instant` ease-out down, `quick` spring-soft back);
//! a ghost circle of a window's chrome dips to 88 % on the same curves; an app icon's tile — the
//! image that IS the control — dips to 92 % on the icon spring both ways, with no layer unless a
//! rounded button holds it (the taskbar's); a row, a tile, a field keep their size and show only
//! the press layer; a control the kit animates a part of (the toggle's thumb) takes neither. Also
//! which controls take the on-fill layer colours: a fill that is one of the theme's tint roles.
//! Pure, host-tested; `probe/anim/interaction.rs` runs it.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the unit tests below.

use crate::hover_wash::{hover_wash_anchor, hover_washable};
use animation::Easing;
use nexus_layout::LayoutBox;
use nexus_layout_types::ShapeKind;
use nexus_theme_tokens::{ColorToken, MotionCurveToken, MotionDurationToken, Tokens};

/// A control is at most this wide and tall to dip: a container catch-all (an overlay backdrop, a
/// panel body) never scales — a scaled 328-px panel displaces its content from its hit boxes.
pub(crate) const CONTROL_MAX_DIM: i32 = 160;
/// The smallest app icon tile (the design's `xs`, 40 px).
const APP_ICON_MIN: i32 = 40;
/// A window's chrome button is at most this wide (the window controls, 36 px).
const CHROME_MAX: i32 = 36;
/// A fill at least this opaque is a FILL (a tint wash — 16 / 30 / 55 % — is not).
const FILL_ALPHA_MIN: u8 = 191;
/// The roles a filled control is filled with (the control system's tints).
const TINTS: [ColorToken; 14] = [
    ColorToken::Accent,
    ColorToken::Danger,
    ColorToken::Success,
    ColorToken::Warning,
    ColorToken::Primary,
    ColorToken::Label,
    ColorToken::ModeAirplane,
    ColorToken::ModeDesktop,
    ColorToken::ModeTablet,
    ColorToken::Chart1,
    ColorToken::Chart2,
    ColorToken::Chart3,
    ColorToken::Chart4,
    ColorToken::Chart5,
];

/// The press feedback of a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PressKind {
    /// An action control (capsule or circle): 95 %.
    Control,
    /// A ghost circle of a window's chrome (the window buttons, a sheet's close): 88 %.
    Chrome,
    /// An app icon: its tile 92 % on the icon spring.
    AppIcon,
    /// The kit animates a part of it (the toggle's thumb).
    Part,
    /// A row, a tile, a field: the press layer only.
    Plain,
}

impl PressKind {
    /// The scale the press dips to; `None`: the size stays.
    pub(crate) fn scale(self) -> Option<f32> {
        match self {
            PressKind::Control => Some(0.95),
            PressKind::Chrome => Some(0.88),
            PressKind::AppIcon => Some(0.92),
            PressKind::Part | PressKind::Plain => None,
        }
    }

    /// How the dip goes down: its motion token and curve (the press hits at once, an icon
    /// springs).
    pub(crate) fn down(self) -> (MotionDurationToken, Easing) {
        match self {
            PressKind::AppIcon => (MotionDurationToken::Quick, spring_icon()),
            _ => (MotionDurationToken::Instant, Easing::EaseOut),
        }
    }

    /// How it comes back on the release: a soft overshoot, an icon's stronger one.
    pub(crate) fn up(self) -> (MotionDurationToken, Easing) {
        match self {
            PressKind::AppIcon => (MotionDurationToken::Quick, spring_icon()),
            _ => (MotionDurationToken::Quick, Easing::Curve(MotionCurveToken::SpringSoft)),
        }
    }
}

fn spring_icon() -> Easing {
    Easing::Curve(MotionCurveToken::SpringIcon)
}

/// What a press on a handler does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PressPlan {
    pub(crate) kind: PressKind,
    /// The node the dip scales: the control, or the app icon's tile.
    pub(crate) target: Option<usize>,
    /// The node the hover and press layers cover; `None`: no layer.
    pub(crate) anchor: Option<usize>,
}

/// The plan for a press on (or the hover of) handler box `handler`: `part` — the kit animates a
/// part of it; `window` — the surface is a window (the desktop's ghost circles are the shell's
/// buttons, a window's are its chrome).
#[must_use]
pub(crate) fn plan(boxes: &[LayoutBox], handler: usize, part: bool, window: bool) -> PressPlan {
    let nothing = |kind| PressPlan { kind, target: None, anchor: None };
    let Some(h) = find(boxes, handler) else { return nothing(PressKind::Plain) };
    if part {
        return nothing(PressKind::Part);
    }
    // A container catch-all (an overlay's backdrop, a panel body) takes taps, never a layer.
    let anchor = hover_wash_anchor(boxes, handler).filter(|_| hover_washable(boxes, handler));
    if let Some(tile) = app_icon_tile(boxes, h) {
        // The icon carries the press; only a rounded button around it shows the layers.
        let layered = anchor.filter(|&a| find(boxes, a).is_some_and(|b| radius(b) > 0));
        return PressPlan { kind: PressKind::AppIcon, target: Some(tile), anchor: layered };
    }
    let sized = h.rect.width.0 <= CONTROL_MAX_DIM && h.rect.height.0 <= CONTROL_MAX_DIM;
    let kind = match anchor.and_then(|a| find(boxes, a)) {
        Some(a) if sized && round(a) => {
            if window && ghost(a) && a.rect.width.0 <= CHROME_MAX {
                PressKind::Chrome
            } else {
                PressKind::Control
            }
        }
        _ => PressKind::Plain,
    };
    let target = matches!(kind, PressKind::Control | PressKind::Chrome).then_some(handler);
    PressPlan { kind, target, anchor }
}

/// Whether control box `b` is FILLED — its fill one of the theme's tint roles, near opaque — so
/// its layers lighten instead (the design's filled buttons, an on toggle-button, a selected chip,
/// the filled play disc).
#[must_use]
pub(crate) fn on_fill(b: &LayoutBox, tokens: &dyn Tokens) -> bool {
    let Some(c) = b.visual.background.filter(|c| c.a >= FILL_ALPHA_MIN) else { return false };
    TINTS.iter().any(|&role| {
        let t = tokens.color(role);
        (t.r, t.g, t.b) == (c.r, c.g, c.b)
    })
}

/// The toggle thumb's press stretch for a thumb `w` px wide (the design: it grows by
/// `round(w · 0.3)` along the travel, anchored to its side): `(scale, grow px)`.
#[must_use]
pub(crate) fn thumb_stretch(w: i32) -> (f32, i32) {
    let w = w.max(1);
    let grow = (w * 3 + 5) / 10;
    ((w + grow) as f32 / w as f32, grow)
}

fn find(boxes: &[LayoutBox], node_id: usize) -> Option<&LayoutBox> {
    boxes.iter().find(|b| b.node_id == node_id)
}

fn radius(b: &LayoutBox) -> i32 {
    b.visual.corner_radius.top_left.0
}

/// A capsule or a circle: the action control's shape.
fn round(b: &LayoutBox) -> bool {
    let half = b.rect.width.0.min(b.rect.height.0) / 2;
    half > 0 && (radius(b) >= half - 1 || matches!(b.visual.shape, ShapeKind::Circle))
}

/// No fill of its own: a ghost.
fn ghost(b: &LayoutBox) -> bool {
    b.visual.background.is_none_or(|c| c.a == 0)
        && b.visual.background_gradient.is_none()
        && b.visual.material.is_none()
}

/// The app icon tile inside handler box `h`: an image (a raster sprite) that is most of the
/// control — square, at least an `xs` tile, at least a third of the control's width (a 24-px file
/// icon in a row is decoration, not the control).
fn app_icon_tile(boxes: &[LayoutBox], h: &LayoutBox) -> Option<usize> {
    let (hx, hy, hw, hh) = (h.rect.x.0, h.rect.y.0, h.rect.width.0, h.rect.height.0);
    boxes
        .iter()
        .find(|b| {
            let (x, y, w, bh) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
            b.node_id >= h.node_id
                && matches!(b.visual.shape, ShapeKind::Raster { .. })
                && x >= hx
                && y >= hy
                && x + w <= hx + hw
                && y + bh <= hy + hh
                && w >= APP_ICON_MIN
                && (w - bh).abs() <= 1
                && w * 3 >= hw
        })
        .map(|b| b.node_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{CornerRadius, FxPx, Rect, Rgba8};
    use nexus_theme_tokens::{DarkTokens, LightTokens};

    fn boxed(node_id: usize, (x, y, w, h): (i32, i32, i32, i32), radius: i32) -> LayoutBox {
        let mut b = LayoutBox {
            node_id,
            rect: Rect {
                x: FxPx::new(x),
                y: FxPx::new(y),
                width: FxPx::new(w),
                height: FxPx::new(h),
            },
            ..LayoutBox::default()
        };
        b.visual.corner_radius = CornerRadius::uniform(FxPx::new(radius));
        b
    }

    fn filled(mut b: LayoutBox, c: Rgba8) -> LayoutBox {
        b.visual.background = Some(c);
        b
    }

    fn sprite(node_id: usize, (x, y, side): (i32, i32, i32)) -> LayoutBox {
        let mut b = boxed(node_id, (x, y, side, side), 0);
        b.visual.shape = ShapeKind::Raster { w: 1, h: 1, rgba: &[0, 0, 0, 255] };
        b
    }

    const GLASS: Rgba8 = Rgba8::new(255, 255, 255, 153);

    /// A capsule button dips to 95 % — instant ease-out down, quick spring-soft back — and both
    /// layers cover it.
    #[test]
    fn a_capsule_button_dips_to_95() {
        let boxes = [filled(boxed(3, (0, 0, 96, 36), 9999), GLASS)];
        let p = plan(&boxes, 3, false, true);
        assert_eq!(p, PressPlan { kind: PressKind::Control, target: Some(3), anchor: Some(3) });
        assert_eq!(p.kind.scale(), Some(0.95));
        assert_eq!(p.kind.down(), (MotionDurationToken::Instant, Easing::EaseOut));
        assert_eq!(
            p.kind.up(),
            (MotionDurationToken::Quick, Easing::Curve(MotionCurveToken::SpringSoft))
        );
    }

    /// The window kit's documented shape — a bare handler around a pill: the pill takes the
    /// layers, the wrapper the dip (it hugs the pill).
    #[test]
    fn a_bare_wrapper_dips_and_its_pill_takes_the_layers() {
        let boxes = [boxed(1, (10, 10, 66, 36), 0), filled(boxed(2, (10, 10, 66, 36), 18), GLASS)];
        let p = plan(&boxes, 1, false, true);
        assert_eq!(p, PressPlan { kind: PressKind::Control, target: Some(1), anchor: Some(2) });
    }

    /// A ghost circle in a window is its chrome (88 %); on the desktop the same circle is a
    /// shell button (95 %).
    #[test]
    fn a_ghost_circle_is_chrome_in_a_window_and_a_button_on_the_desktop() {
        let boxes = [boxed(5, (0, 0, 30, 30), 15)];
        assert_eq!(plan(&boxes, 5, false, true).kind, PressKind::Chrome);
        assert_eq!(PressKind::Chrome.scale(), Some(0.88));
        assert_eq!(plan(&boxes, 5, false, false).kind, PressKind::Control);
        // A filled circle is an action control wherever it is.
        let filled_circle = [filled(boxed(5, (0, 0, 36, 36), 18), GLASS)];
        assert_eq!(plan(&filled_circle, 5, false, true).kind, PressKind::Control);
    }

    /// A row keeps its size: the press layer only.
    #[test]
    fn a_rounded_row_shows_only_the_press_layer() {
        let boxes = [boxed(1, (0, 0, 680, 41), 10)];
        let p = plan(&boxes, 1, false, true);
        assert_eq!(p, PressPlan { kind: PressKind::Plain, target: None, anchor: Some(1) });
        assert_eq!(p.kind.scale(), None);
    }

    /// A launcher cell: the icon's tile dips to 92 % on the icon spring both ways, the label
    /// stays, and nothing washes the cell (the design: hover changes nothing).
    #[test]
    fn an_app_icon_dips_its_tile_and_takes_no_layer() {
        let boxes =
            [boxed(1, (0, 0, 162, 92), 0), boxed(2, (53, 0, 56, 92), 0), sprite(3, (53, 0, 56))];
        let p = plan(&boxes, 1, false, false);
        assert_eq!(p, PressPlan { kind: PressKind::AppIcon, target: Some(3), anchor: None });
        assert_eq!(p.kind.scale(), Some(0.92));
        let icon = Easing::Curve(MotionCurveToken::SpringIcon);
        assert_eq!(p.kind.down(), (MotionDurationToken::Quick, icon));
        assert_eq!(p.kind.up(), (MotionDurationToken::Quick, icon));
    }

    /// The taskbar's app button: a rounded button around the icon shows the layers, and the
    /// icon still dips.
    #[test]
    fn a_rounded_app_button_shows_layers_and_dips_its_icon() {
        let boxes = [boxed(1, (0, 0, 48, 50), 12), sprite(2, (4, 4, 40))];
        let p = plan(&boxes, 1, false, false);
        assert_eq!(p, PressPlan { kind: PressKind::AppIcon, target: Some(2), anchor: Some(1) });
    }

    /// A file icon in a row is decoration: the row is the control.
    #[test]
    fn a_small_image_in_a_row_is_not_an_app_icon() {
        let boxes = [boxed(1, (0, 0, 240, 40), 8), sprite(2, (8, 8, 24))];
        assert_eq!(plan(&boxes, 1, false, true).kind, PressKind::Plain);
        // Nor is a 40-px app icon at the start of a long list row.
        let row = [boxed(1, (0, 0, 300, 56), 12), sprite(2, (8, 8, 40))];
        assert_eq!(plan(&row, 1, false, true).kind, PressKind::Plain);
    }

    /// A container catch-all takes the tap and nothing else: no layer, no dip.
    #[test]
    fn a_container_takes_no_layer_and_no_dip() {
        let boxes = [boxed(1, (0, 0, 960, 570), 0)];
        assert_eq!(
            plan(&boxes, 1, false, true),
            PressPlan { kind: PressKind::Plain, target: None, anchor: None }
        );
        // A big capsule is no control to dip either (it still takes the layers).
        let wide = [boxed(2, (0, 0, 400, 44), 22)];
        assert_eq!(plan(&wide, 2, false, true).kind, PressKind::Plain);
    }

    /// The toggle's switch: the kit animates its thumb — no layer, no dip of the track.
    #[test]
    fn a_part_pressed_control_takes_neither() {
        let boxes = [boxed(1, (0, 0, 44, 26), 13), boxed(2, (3, 3, 20, 20), 10)];
        assert_eq!(
            plan(&boxes, 1, true, true),
            PressPlan { kind: PressKind::Part, target: None, anchor: None }
        );
        assert_eq!(PressKind::Part.scale(), None);
    }

    /// A stale handler id (the scene changed under the pointer) plans nothing.
    #[test]
    fn a_stale_handler_plans_nothing() {
        let boxes = [boxed(1, (0, 0, 30, 30), 15)];
        assert_eq!(plan(&boxes, 9, false, true).anchor, None);
    }

    /// A filled control is one whose fill is a tint role, near opaque: accent, label (the play
    /// disc) — not glass, not a tint wash, not a bare box.
    #[test]
    fn a_tint_fill_is_on_fill_and_glass_is_not() {
        for tokens in [&LightTokens as &dyn Tokens, &DarkTokens] {
            let accent = tokens.color(ColorToken::Accent);
            let label = tokens.color(ColorToken::Label);
            let b = boxed(1, (0, 0, 80, 36), 18);
            assert!(on_fill(&filled(b.clone(), accent), tokens));
            assert!(on_fill(&filled(b.clone(), label), tokens));
            let wash = Rgba8::new(accent.r, accent.g, accent.b, 41);
            assert!(!on_fill(&filled(b.clone(), wash), tokens));
            assert!(!on_fill(&filled(b.clone(), tokens.color(ColorToken::GlassCard)), tokens));
            assert!(!on_fill(&b, tokens));
        }
    }

    /// The thumb grows by round(w · 0.3): md 20 → 26 (1.30), xs 15 → 20, xl 29 → 38.
    #[test]
    fn the_thumb_stretch_follows_its_width() {
        assert_eq!(thumb_stretch(20), (1.3, 6));
        assert_eq!(thumb_stretch(15).1, 5);
        assert_eq!(thumb_stretch(29).1, 9);
        assert_eq!(thumb_stretch(25).1, 8);
    }
}
