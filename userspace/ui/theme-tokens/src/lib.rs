// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! Semantic **design tokens** — the interface through which a theme supplies its values to the
//! DSL, the primitives and the shell (RFC-0116: the design artifacts' one token set).
//!
//! Tokens are **enum keys**, not strings: deterministic, `no_std`, a 1:1 target for the DSL
//! (`.fg(label)` is [`ColorToken::Label`]). A theme is a [`Tokens`] implementation — the colour of
//! every role, every shadow, every backdrop; the scales (text styles, radii, gaps, control
//! heights, motion durations) are theme-invariant free functions. Swapping the theme (light,
//! dark, high contrast, a user accent) is swapping the `Tokens` impl — no widget code changes.
//!
//! The authoring side (`resources/themes/*.nxtheme.toml`, the `nexus-theme` std crate) is
//! compiled into these snapshots by `build.rs`.

mod color_token;
mod design;
pub use color_token::ColorToken;
pub use design::{
    concentric, on_spacing_grid, recipe, Backdrop, BackdropToken, ControlSize, MaterialRecipe,
    RadiusToken, ShadowSpec, ShadowToken, SpaceToken, TextStyleSpec, TextStyleToken, HAIRLINE,
    SPACING_GRID,
};
use nexus_layout_types::{FontWeight, FxPx, Rgba8};

/// Motion duration step (design motion scale, ms). Themable so a
/// reduced-motion theme can zero every step; authored in `[motion]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionDurationToken {
    /// 100ms — press-down feedback.
    Instant,
    /// 160ms — hover, color/background shifts.
    Swift,
    /// 280ms — toggles, small controls.
    Quick,
    /// 400ms — collapse, dismiss.
    Base,
    /// 500ms — spring expand, window entry.
    Slow,
}

/// Motion easing curve (design motion vocabulary). The control points are the
/// theme-invariant cubic-beziers from `reference/tokens/motion.css` — one
/// physics vocabulary for the whole OS: enter/expand springs, exits are smooth
/// and quicker, micro-feedback is swift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionCurveToken {
    /// Panel/expand entry, elastic overshoot.
    Spring,
    /// Subtle overshoot: switch thumbs, chips.
    SpringSoft,
    /// Icon pop, strongest overshoot.
    SpringIcon,
    /// Collapse, exit, fades — never bouncy on the way out.
    Smooth,
    /// Page swipes, large moves.
    Glide,
    /// A surface's entrance: a fast arrival and a long soft settle — the panels' and the
    /// launcher's (design artifacts: panels, `cubic-bezier(0.16, 1, 0.3, 1)`).
    Enter,
}

impl MotionCurveToken {
    /// The `cubic-bezier(x1, y1, x2, y2)` control points.
    pub const fn control_points(self) -> [f32; 4] {
        match self {
            MotionCurveToken::Spring => [0.34, 1.4, 0.5, 1.0],
            MotionCurveToken::SpringSoft => [0.34, 1.2, 0.5, 1.0],
            MotionCurveToken::SpringIcon => [0.34, 1.56, 0.64, 1.0],
            MotionCurveToken::Smooth => [0.4, 0.0, 0.2, 1.0],
            MotionCurveToken::Glide => [0.22, 1.0, 0.36, 1.0],
            MotionCurveToken::Enter => [0.16, 1.0, 0.3, 1.0],
        }
    }
}

/// A theme: the colour of every role, every shadow and every backdrop under it.
pub trait Tokens {
    fn color(&self, token: ColorToken) -> Rgba8;
    fn shadow(&self, token: ShadowToken) -> ShadowSpec;
    fn backdrop(&self, token: BackdropToken) -> Backdrop;

    /// A motion duration (ms); a reduced-motion theme zeroes them.
    fn motion_ms(&self, token: MotionDurationToken) -> u32 {
        motion_duration_ms(token)
    }
}

// The snapshots and scales generated from `resources/themes/*.nxtheme.toml`:
// `{base,dark,light,highcontrast}_{color,shadow,backdrop}`, `radius_px`, `space_px`,
// `control_height_px`, `text_style_spec`, `motion_duration_ms`.
include!(concat!(env!("OUT_DIR"), "/generated_tokens.rs"));

/// The light theme (the artifacts' light values; also the fallback for an unknown mode).
#[derive(Debug, Clone, Copy, Default)]
pub struct BaseTokens;

impl Tokens for BaseTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        base_color(token)
    }
    fn shadow(&self, token: ShadowToken) -> ShadowSpec {
        base_shadow(token)
    }
    fn backdrop(&self, token: BackdropToken) -> Backdrop {
        base_backdrop(token)
    }
}

/// The dark theme.
#[derive(Debug, Clone, Copy, Default)]
pub struct DarkTokens;

impl Tokens for DarkTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        dark_color(token)
    }
    fn shadow(&self, token: ShadowToken) -> ShadowSpec {
        dark_shadow(token)
    }
    fn backdrop(&self, token: BackdropToken) -> Backdrop {
        dark_backdrop(token)
    }
}

/// The light theme as selected by the user.
#[derive(Debug, Clone, Copy, Default)]
pub struct LightTokens;

impl Tokens for LightTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        light_color(token)
    }
    fn shadow(&self, token: ShadowToken) -> ShadowSpec {
        light_shadow(token)
    }
    fn backdrop(&self, token: BackdropToken) -> Backdrop {
        light_backdrop(token)
    }
}

/// The high-contrast theme: black and white, strong edges, no blur.
#[derive(Debug, Clone, Copy, Default)]
pub struct HighContrastTokens;

impl Tokens for HighContrastTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        highcontrast_color(token)
    }
    fn shadow(&self, token: ShadowToken) -> ShadowSpec {
        highcontrast_shadow(token)
    }
    fn backdrop(&self, token: BackdropToken) -> Backdrop {
        highcontrast_backdrop(token)
    }
}

/// The curated **accent palette** (settings → Erscheinungsbild → Farbpalette).
/// Index 0 = the theme's built-in accent (no override); 1..=n are the named
/// alternates as `(name, dark-mode value, light-mode value)` — the dark value
/// is one step lighter for contrast on dark surfaces, mirroring how the
/// built-in blue differs between the two themes.
///
/// **APPEND-ONLY.** A palette entry's position IS its wire index (one-based:
/// `accent_index` returns the position plus one), and windowd packs that index
/// into the theme push — so reordering would repaint every persisted
/// `ui.theme.accent`. Appending is safe because the persisted value is the
/// NAME; the index is recomputed on every apply.
///
/// The nine entries are the settings design artifacts's palette; its "Nexus
/// Blau" IS the theme's built-in accent, so it rides index 0 (`"default"`,
/// also spelled `"blue"` — see `accent_index`).
pub const ACCENT_PALETTE: &[(&str, Rgba8, Rgba8)] = &[
    ("violet", Rgba8::new(167, 139, 250, 255), Rgba8::new(139, 92, 246, 255)),
    ("pink", Rgba8::new(244, 114, 182, 255), Rgba8::new(236, 72, 153, 255)),
    ("red", Rgba8::new(248, 113, 113, 255), Rgba8::new(239, 68, 68, 255)),
    ("orange", Rgba8::new(251, 146, 60, 255), Rgba8::new(249, 115, 22, 255)),
    ("green", Rgba8::new(74, 222, 128, 255), Rgba8::new(34, 197, 94, 255)),
    ("teal", Rgba8::new(45, 212, 191, 255), Rgba8::new(20, 184, 166, 255)),
    ("amber", Rgba8::new(251, 191, 36, 255), Rgba8::new(245, 158, 11, 255)),
    ("graphite", Rgba8::new(148, 163, 184, 255), Rgba8::new(100, 116, 139, 255)),
];

/// The accent override for palette index `1..=ACCENT_PALETTE.len()` in the
/// given mode; `None` for 0/out-of-range (= the theme's built-in accent).
#[must_use]
pub const fn accent_color(index: u8, dark: bool) -> Option<Rgba8> {
    if index == 0 || index as usize > ACCENT_PALETTE.len() {
        return None;
    }
    let (_, d, l) = ACCENT_PALETTE[index as usize - 1];
    Some(if dark { d } else { l })
}

/// Palette index for an accent NAME (`"violet"` → 1); `None` for unknown
/// names, `Some(0)` for `"default"`/`""` (the built-in accent).
#[must_use]
pub fn accent_index(name: &str) -> Option<u8> {
    if name.is_empty() || name == "default" || name == "blue" {
        return Some(0);
    }
    let mut i = 0;
    while i < ACCENT_PALETTE.len() {
        if ACCENT_PALETTE[i].0.as_bytes() == name.as_bytes() {
            return Some(i as u8 + 1);
        }
        i += 1;
    }
    None
}

/// A theme with the user's accent: `Accent` takes the chosen hue and `AccentGlow` follows it at
/// the role's own alpha, so an orange accent glows orange.
#[derive(Clone, Copy)]
pub struct AccentTokens {
    pub base: &'static (dyn Tokens + Sync),
    pub accent: Rgba8,
}

impl Tokens for AccentTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        match token {
            ColorToken::Accent => self.accent,
            ColorToken::AccentGlow => {
                let a = self.base.color(ColorToken::AccentGlow).a;
                Rgba8::new(self.accent.r, self.accent.g, self.accent.b, a)
            }
            other => self.base.color(other),
        }
    }
    fn shadow(&self, token: ShadowToken) -> ShadowSpec {
        self.base.shadow(token)
    }
    fn backdrop(&self, token: BackdropToken) -> Backdrop {
        self.base.backdrop(token)
    }
    fn motion_ms(&self, token: MotionDurationToken) -> u32 {
        self.base.motion_ms(token)
    }
}

/// A theme without translucency (the reduce-transparency setting, as the reference systems do it):
/// no backdrop blurs, and the glass roles become two solid grey surfaces taken from the theme
/// itself — `surfaceOverlay` and `fill` composited over it. Panels and the window glass take the
/// darker of the two, cards the lighter, so a card stays visible on its panel in both themes: light
/// grey panels with near-white cards in light mode, dark grey panels with lighter grey cards in dark
/// mode — never a white slab. (A glass tint's colour at full alpha is not its solid form: dark
/// `glassPanel` is a white veil over the blur and would turn every dark panel white.)
#[derive(Clone, Copy)]
pub struct SolidTokens {
    pub base: &'static (dyn Tokens + Sync),
}

/// `top` over the opaque `bottom`, opaque.
fn over(top: Rgba8, bottom: Rgba8) -> Rgba8 {
    let a = u32::from(top.a);
    let mix = |t: u8, b: u8| ((u32::from(t) * a + u32::from(b) * (255 - a) + 127) / 255) as u8;
    Rgba8::new(mix(top.r, bottom.r), mix(top.g, bottom.g), mix(top.b, bottom.b), 255)
}

/// Perceived brightness (Rec. 601 luma), 0..=255.
fn luma(c: Rgba8) -> u32 {
    (u32::from(c.r) * 299 + u32::from(c.g) * 587 + u32::from(c.b) * 114) / 1000
}

impl SolidTokens {
    /// The two solid surfaces: (the darker — panels and windows, the lighter — cards).
    fn surfaces(&self) -> (Rgba8, Rgba8) {
        let c = self.base.color(ColorToken::SurfaceOverlay);
        let overlay = Rgba8::new(c.r, c.g, c.b, 255);
        let grey = over(self.base.color(ColorToken::Fill), overlay);
        if luma(grey) <= luma(overlay) {
            (grey, overlay)
        } else {
            (overlay, grey)
        }
    }
}

impl Tokens for SolidTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        match token {
            ColorToken::GlassPanel | ColorToken::GlassWindow => self.surfaces().0,
            // A floating surface reads against the window under it: the lighter grey.
            ColorToken::GlassCard | ColorToken::GlassOverlay => self.surfaces().1,
            other => self.base.color(other),
        }
    }
    fn shadow(&self, token: ShadowToken) -> ShadowSpec {
        self.base.shadow(token)
    }
    fn backdrop(&self, token: BackdropToken) -> Backdrop {
        Backdrop { blur: 0, ..self.base.backdrop(token) }
    }
    fn motion_ms(&self, token: MotionDurationToken) -> u32 {
        self.base.motion_ms(token)
    }
}

/// The weight a text style sets unless the author overrides it.
#[must_use]
pub fn style_weight(style: TextStyleToken) -> FontWeight {
    style.spec().weight
}

#[cfg(test)]
mod tests;
