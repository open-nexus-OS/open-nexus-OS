// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The closed token vocabularies a modifier may name (RFC-0116: one token set), and the names
//! that set retired with what replaces each.
//!
//! The runtime turns the accepted names into values (`nexus-dsl-runtime` `registry::tokens`);
//! its lock-step test pins this table against the typed tokens (`nexus-theme-tokens`). An
//! unknown name is a compile error; a retired one names its replacement in the message.

/// Semantic **colour roles** a `.fg`/`.bg`/`.borderColor`/`.bgFade`/`.tint`/`.borderTint` may
/// name — exactly `nexus_theme_tokens::ColorToken::ALL` by name, in its order.
pub const COLOR_TOKENS: &[&str] = &[
    "background",
    "label",
    "labelSecondary",
    "labelTertiary",
    "separator",
    "fill",
    "fillHover",
    "fillActive",
    "glassPanel",
    "glassWindow",
    "glassCard",
    "glassOverlay",
    "glassEdge",
    "surfacePane",
    "surfaceOverlay",
    "scrim",
    "primary",
    "onPrimary",
    "accent",
    "accentGlow",
    "onAccent",
    "danger",
    "success",
    "warning",
    "modeAirplane",
    "modeDesktop",
    "modeTablet",
    "island",
    "chart1",
    "chart2",
    "chart3",
    "chart4",
    "chart5",
    "wallpaperTint",
    "labelShadow",
    "transparent",
];

/// The modifiers whose arguments are colour roles.
pub const COLOR_MODIFIERS: &[&str] = &["fg", "bg", "borderColor", "bgFade", "tint", "borderTint"];

/// The thirteen text styles `.textStyle` names (size, line height, weight and tracking together);
/// `clock` is the lock screen's time.
pub const TEXT_STYLES: &[&str] = &[
    "clock",
    "display",
    "largeTitle",
    "title1",
    "title2",
    "title3",
    "headline",
    "body",
    "subheadline",
    "footnote",
    "caption",
    "caption2",
    "overline",
];

/// The tint wash strengths of `.tint(role, level)`: `fill` 16 % (the default), `border` 30 %,
/// `line` 55 % — the only alphas a role takes.
pub const TINT_LEVELS: &[&str] = &["fill", "border", "line"];

/// The 4-pt grid in px a spacing modifier's number must be on (6 only between an icon and its
/// label).
pub const SPACING_GRID: &[i64] = &[0, 2, 4, 6, 8, 12, 16, 20, 24, 32, 40, 48];

/// The semantic spacing names: `gapInline` 8, `gapGroup` 12, `gapSection` 24, `marginLayout` 16.
pub const SPACING_TOKENS: &[&str] = &["gapInline", "gapGroup", "gapSection", "marginLayout"];

/// The modifiers whose argument is a spacing on the grid (px).
pub const SPACING_MODIFIERS: &[&str] = &[
    "padding",
    "paddingX",
    "paddingY",
    "paddingTop",
    "paddingBottom",
    "paddingLeading",
    "paddingTrailing",
    "gap",
    "margin",
    "rowGap",
    "hitSlop",
];

/// The radii: `sm` 8, `md` 12, `lg` 16, `xl` 24, `full`.
pub const RADII: &[&str] = &["sm", "md", "lg", "xl", "full"];

/// The materials: only a surface on the wallpaper blurs (`panel`, `window`, `dock`, `thin`,
/// `ultraThin`); `card` is translucent without blur; `pane` and `overlay` are opaque.
pub const MATERIALS: &[&str] =
    &["panel", "window", "dock", "thin", "ultraThin", "card", "pane", "overlay"];

/// The materials that blur what lies behind them — one per stack.
pub const BLUR_MATERIALS: &[&str] = &["panel", "window", "dock", "thin", "ultraThin"];

/// Closed token vocabularies per modifier: `(modifier, allowed tokens)` — the default for every
/// argument; [`token_vocabulary_at`] refines a position.
///
/// Only vocabularies that are genuinely closed belong here. A size modifier (`.width(320)`) takes
/// raw px and is absent on purpose; a spacing modifier takes a grid number or a spacing name.
pub const TOKEN_VOCABULARIES: &[(&str, &[&str])] = &[
    ("fg", COLOR_TOKENS),
    ("bg", COLOR_TOKENS),
    ("borderColor", COLOR_TOKENS),
    ("bgFade", COLOR_TOKENS),
    ("tint", COLOR_TOKENS),
    ("borderTint", COLOR_TOKENS),
    ("textStyle", TEXT_STYLES),
    ("fontWeight", &["light", "normal", "medium", "semibold", "bold"]),
    ("textAlign", &["left", "center", "right"]),
    ("figures", &["proportional", "tabular"]),
    ("material", MATERIALS),
    ("rounded", RADII),
    ("corners", RADII),
    // `halo`: the `labelShadow` behind a text that sits on the wallpaper (the top bar).
    (
        "shadow",
        &["shadow1", "shadow2", "shadow3", "shadow4", "shadowInset", "edgeHighlight", "halo"],
    ),
    ("border", &["hairline", "ring"]),
    ("align", &["start", "center", "end", "stretch"]),
    ("justify", &["start", "center", "end", "between", "around"]),
    ("direction", &["row", "column"]),
    ("overflow", &["visible", "hidden"]),
    ("scroll", &["vertical", "horizontal", "paged"]),
    ("overlay", &["modal", "transient"]),
    ("padding", SPACING_TOKENS),
    ("paddingX", SPACING_TOKENS),
    ("paddingY", SPACING_TOKENS),
    ("paddingTop", SPACING_TOKENS),
    ("paddingBottom", SPACING_TOKENS),
    ("paddingLeading", SPACING_TOKENS),
    ("paddingTrailing", SPACING_TOKENS),
    ("gap", SPACING_TOKENS),
    ("margin", SPACING_TOKENS),
    ("hitSlop", SPACING_TOKENS),
];

/// The closed token vocabulary of `name`, if it has one.
#[must_use]
pub fn token_vocabulary(name: &str) -> Option<&'static [&'static str]> {
    TOKEN_VOCABULARIES.iter().find(|(m, _)| *m == name).map(|(_, tokens)| *tokens)
}

/// The vocabulary of argument `index` of `name`: `.tint(role, level)` takes a strength second,
/// every other modifier one vocabulary for all its arguments.
#[must_use]
pub fn token_vocabulary_at(name: &str, index: usize) -> Option<&'static [&'static str]> {
    match (name, index) {
        ("tint", 1) => Some(TINT_LEVELS),
        _ => token_vocabulary(name),
    }
}

/// Modifiers RFC-0116 retired (their ids stay in the catalog — ids are positional — and any use
/// is rejected with this replacement).
pub const RETIRED_MODIFIERS: &[(&str, &str)] = &[
    ("textSize", "use `.textStyle(…)`: one of the thirteen text styles sets size, line and weight"),
    ("leading", "the text style sets the line height: use `.textStyle(…)`"),
    ("textShadow", "a label on the wallpaper takes `.shadow(halo)` (the `labelShadow` role)"),
];

/// The replacement of a retired modifier, if `name` is one.
#[must_use]
pub fn retired_modifier(name: &str) -> Option<&'static str> {
    RETIRED_MODIFIERS.iter().find(|(m, _)| *m == name).map(|(_, hint)| *hint)
}

/// Names RFC-0116 retired, with what replaces each: `(vocabulary, old, replacement)`. The
/// vocabulary is `color` for every colour modifier, otherwise the modifier's name.
pub const RETIRED_TOKENS: &[(&str, &str, &str)] = &[
    (
        "color",
        "surface",
        "`background` (a page) · `surfacePane` (inside a window) · `surfaceOverlay` (floating) · \
         `glassCard` (on glass)",
    ),
    ("color", "surfaceVariant", "`fill` (a control's resting fill) · `separator` (a hairline)"),
    ("color", "onSurface", "`label`"),
    ("color", "onSurfaceVariant", "`labelSecondary`"),
    ("color", "border", "`separator` (a hairline) · `glassEdge` (the edge of a glass layer)"),
    ("color", "divider", "`separator`"),
    ("color", "islandBg", "`island`"),
    ("color", "onDanger", "`onAccent`"),
    ("color", "onSuccess", "`onAccent`"),
    ("color", "onInfo", "`onAccent`"),
    ("color", "onDestructive", "`onAccent`"),
    ("color", "onWarning", "a `warning` label on `.tint(warning)`: no filled warning surface"),
    ("color", "info", "`accent`"),
    ("color", "focusRing", "`accent`"),
    ("color", "toggleOnBg", "`accent`"),
    ("color", "destructive", "`danger`"),
    ("color", "notifDot", "`danger`"),
    ("color", "shadow", "`.shadow(shadow1 … shadow4)`"),
    ("color", "onGlass", "`label`"),
    ("color", "onGlassStrong", "`label`"),
    ("color", "glassIcon", "`label` (an inactive icon: `labelSecondary`)"),
    ("color", "onGlassMuted", "`labelSecondary`"),
    ("color", "glassPlaceholder", "`labelTertiary`"),
    ("color", "glassFocus", "`glassEdge` (a hairline) · `accent` (a focus ring)"),
    ("color", "glassFill", "`fill`"),
    ("color", "glassFillStrong", "`fill`"),
    ("color", "toggleOffBg", "`fill`"),
    ("color", "sliderTrack", "`fill`"),
    ("color", "sliderFill", "`onAccent` (the white knob) with `.shadow(shadow1)`"),
    ("color", "sliderIcon", "`labelSecondary`"),
    ("color", "glassHover", "`fill`: hover is the host's `fillHover` layer, never a background"),
    ("color", "glassActive", "`fill`: press is the host's `fillActive` layer, never a background"),
    ("color", "accentSoft", "`.tint(accent)`: the accent wash"),
    ("color", "wallpaperVignette", "`scrim`, or no fade"),
    ("color", "textShadow", "`.shadow(halo)` on a label that sits on the wallpaper"),
    ("color", "textShadowStrong", "`.shadow(halo)` on a label that sits on the wallpaper"),
    ("material", "opaque", "no material: an opaque fill is a `.bg(…)`"),
    ("material", "subtle", "`.bg(fill)` (an icon circle) · `card` (a grouped row on glass)"),
    ("material", "windowPane", "`pane`"),
    ("material", "windowBar", "`overlay` with `.shadow(shadow3)`"),
    ("rounded", "xxl", "`lg` (16): the radii are sm 8, md 12, lg 16, xl 24, full"),
    ("shadow", "sm", "`shadow1`"),
    ("shadow", "md", "`shadow2`"),
    ("shadow", "lg", "`shadow2` · `shadow3`"),
    ("shadow", "xl", "`shadow3` · `shadow4`"),
    ("shadow", "xxl", "`shadow4`"),
    ("border", "thin", "`hairline`"),
    ("border", "medium", "`ring` (2 px)"),
    ("border", "thick", "`ring`: no 3 px border exists"),
    ("fontWeight", "regular", "`normal`"),
];

/// The replacement of a retired token `name` under `modifier`, if it is one.
#[must_use]
pub fn retired_token(modifier: &str, name: &str) -> Option<&'static str> {
    let vocabulary = if COLOR_MODIFIERS.contains(&modifier) { "color" } else { modifier };
    RETIRED_TOKENS
        .iter()
        .find(|(v, old, _)| *v == vocabulary && *old == name)
        .map(|(_, _, hint)| *hint)
}
