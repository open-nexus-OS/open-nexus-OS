// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The frontend's knowledge of the platform surface: widget kinds, the
//! modifier catalog (with field classes), interaction triggers, and the
//! read-only device environment.
//!
//! **SSOT note:** these const tables are the single source the checker, the
//! lowering pass, the runtime's widget registry generator, and the
//! `docs/dev/dsl/modifiers.md` catalog table derive from (docs emission via
//! `nx-dsl` — keeping frontend and runtime structurally unable to disagree).

/// Invalidation class of a modifier/property (docs/dev/dsl/ir.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldClass {
    Layout,
    Paint,
    Semantics,
}

/// Argument shape of a modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModArg {
    /// Semantic token name (spacing step, color role, radius, …).
    Token,
    /// Plain integer (e.g. `.grow(1)`, `.truncate(2)`).
    Int,
    /// Boolean flag.
    Bool,
    /// Arbitrary expression (e.g. `.key(user.id)`).
    Expr,
    /// Translatable string (`.label(@t("…"))` or literal).
    Text,
}

pub struct ModifierSpec {
    pub name: &'static str,
    pub args: &'static [ModArg],
    pub class: FieldClass,
}

/// The modifier catalog (hybrid utility vocabulary, docs/dev/dsl/modifiers.md).
/// Order is the canonical catalog order; `modId` = index into this table.
pub const MODIFIERS: &[ModifierSpec] = &[
    // -- spacing (layout)
    ModifierSpec { name: "padding", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingX", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingY", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingTop", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingBottom", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingLeading", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "paddingTrailing", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "gap", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "margin", args: &[ModArg::Token], class: FieldClass::Layout },
    // -- sizing (layout)
    ModifierSpec { name: "width", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "height", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "minWidth", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "maxWidth", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "minHeight", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "maxHeight", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "grow", args: &[ModArg::Int], class: FieldClass::Layout },
    ModifierSpec { name: "shrink", args: &[ModArg::Int], class: FieldClass::Layout },
    ModifierSpec { name: "aspect", args: &[ModArg::Int, ModArg::Int], class: FieldClass::Layout },
    // -- layout
    ModifierSpec { name: "align", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "justify", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "direction", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "wrap", args: &[ModArg::Bool], class: FieldClass::Layout },
    ModifierSpec { name: "overflow", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "zIndex", args: &[ModArg::Token], class: FieldClass::Layout },
    // -- color & surface (paint)
    ModifierSpec { name: "bg", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "fg", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "borderColor", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "opacity", args: &[ModArg::Int], class: FieldClass::Paint },
    ModifierSpec { name: "material", args: &[ModArg::Token], class: FieldClass::Paint },
    // -- shape & elevation (paint)
    ModifierSpec { name: "rounded", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "border", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "shadow", args: &[ModArg::Token], class: FieldClass::Paint },
    // -- typography (layout: metrics affect measurement)
    ModifierSpec { name: "textSize", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "fontWeight", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "textAlign", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "leading", args: &[ModArg::Token], class: FieldClass::Layout },
    ModifierSpec { name: "truncate", args: &[ModArg::Int], class: FieldClass::Layout },
    // -- interaction
    ModifierSpec { name: "disabled", args: &[ModArg::Bool], class: FieldClass::Paint },
    ModifierSpec { name: "focusable", args: &[ModArg::Bool], class: FieldClass::Semantics },
    ModifierSpec { name: "hitSlop", args: &[ModArg::Token], class: FieldClass::Layout },
    // -- accessibility (semantics)
    ModifierSpec { name: "label", args: &[ModArg::Text], class: FieldClass::Semantics },
    ModifierSpec { name: "role", args: &[ModArg::Token], class: FieldClass::Semantics },
    ModifierSpec { name: "hint", args: &[ModArg::Text], class: FieldClass::Semantics },
    // -- motion (paint)
    ModifierSpec {
        name: "animate",
        args: &[ModArg::Token, ModArg::Expr],
        class: FieldClass::Paint,
    },
    ModifierSpec { name: "transition", args: &[ModArg::Token], class: FieldClass::Paint },
    ModifierSpec { name: "effect", args: &[ModArg::Token, ModArg::Expr], class: FieldClass::Paint },
    // -- identity (layout)
    ModifierSpec { name: "key", args: &[ModArg::Expr], class: FieldClass::Layout },
    // -- scrolling (layout): `.scroll(vertical|horizontal)` marks THIS
    // container as the page's scroll viewport (overflow clipped; the host
    // applies a paint-time offset — pretext: scrolling never re-layouts).
    ModifierSpec { name: "scroll", args: &[ModArg::Token], class: FieldClass::Layout },
    // -- overlay (layout, APPEND-ONLY id): `.overlay()` lifts THIS container
    // OUT OF FLOW as a full-bleed layer over its parent's content (drop-down
    // panels, dialogs — design_handoff_launcher). Anchor inside the layer
    // with ordinary flex (rows/Spacer/justify); paint and hit-testing prefer
    // the layer naturally (later node ids win).
    ModifierSpec { name: "overlay", args: &[], class: FieldClass::Layout },
    // -- gradient fill (paint, APPEND-ONLY id): `.bgGradient(top, bottom)` —
    // a vertical linear background gradient (the design system's
    // `linear-gradient(to bottom, …)`). Args are EXPRESSIONS evaluating to
    // "#rrggbb"/"#rrggbbaa" strings, so both literals and props work
    // (per-app icon artwork colors ride the manifest → enumerate → props).
    ModifierSpec {
        name: "bgGradient",
        args: &[ModArg::Expr, ModArg::Expr],
        class: FieldClass::Paint,
    },
    // -- text shadow (paint, APPEND-ONLY id): `.textShadow(none|soft|strong)`
    // — legibility for text sitting on a wallpaper (RFC-0082). An EMPHASIS
    // step, not a blur radius: the row-based painter draws one extra glyph
    // pass at a 1px offset, because there is no offscreen buffer to blur in.
    ModifierSpec { name: "textShadow", args: &[ModArg::Token], class: FieldClass::Paint },
    // -- token gradient (paint, APPEND-ONLY id): `.bgFade(top, bottom)` — the
    // same vertical fill as `.bgGradient`, but from two COLOR TOKENS so it
    // re-themes. `.bgGradient` keeps taking raw hex because app-icon artwork
    // colors ride the manifest; both forms are legitimate.
    ModifierSpec {
        name: "bgFade",
        args: &[ModArg::Token, ModArg::Token],
        class: FieldClass::Paint,
    },
    // -- grid tracks (layout, APPEND-ONLY ids): `.columns(n)` turns THIS
    // container into an n-column GRID (the engine's `LayoutNode::Grid`,
    // row-major, n equal 1fr tracks). On a `List(...)` the items become the
    // grid cells — the data-driven launcher/workspace grids
    // (design_handoff_launcher §3.2/§9/§11); the `Grid` widget is the
    // static-content sugar over the same lowering. `.gap()` stays the
    // COLUMN gap; `.rowGap(n)` sets the row gap (same spacing scale, defaults
    // to the column gap).
    ModifierSpec { name: "columns", args: &[ModArg::Int], class: FieldClass::Layout },
    ModifierSpec { name: "rowGap", args: &[ModArg::Int], class: FieldClass::Layout },
    // -- flex base size (layout, APPEND-ONLY id): `.basis(n)` is the child's
    // base size on the parent's MAIN axis, replacing its measured content
    // size in the parent's distribution. `.grow()` alone only shares out the
    // LEFTOVER on top of each child's own width, so a keypad row labelled
    // `AC`/`7`/`8`/`9` never divides evenly — `.basis(0).grow(1)` on every
    // child makes the split exact, and `.basis(gap).grow(2)` spans two tracks
    // (the calculator's double-width `0`). Raw px, like `.width`.
    ModifierSpec { name: "basis", args: &[ModArg::Int], class: FieldClass::Layout },
    // `.textFit(pct, min, max)` — BOX-RELATIVE type (append-only id). THIS
    // container derives one font size from its own content-box height and
    // hands it to its text descendants, like an inherited CSS `font-size`.
    // A ratio, not "as large as fits": the calculator handoff puts a 23px
    // label in a ~75px key (~30%). Percent + two raw-px clamps.
    ModifierSpec {
        name: "textFit",
        args: &[ModArg::Int, ModArg::Int, ModArg::Int],
        class: FieldClass::Layout,
    },
];

#[must_use]
pub fn modifier_spec(name: &str) -> Option<(u16, &'static ModifierSpec)> {
    MODIFIERS
        .iter()
        .enumerate()
        .find(|(_, spec)| spec.name == name)
        .map(|(idx, spec)| (idx as u16, spec))
}

pub struct WidgetSpec {
    pub name: &'static str,
    /// Prop the positional sugar fills (`Text("hi")` → `value`).
    pub primary_prop: Option<&'static str>,
    /// Interactive nodes need an accessible name (label prop or `.label()`).
    pub interactive: bool,
    /// The prop that provides the accessible name if present.
    pub label_prop: Option<&'static str>,
    pub allows_children: bool,
}

/// v0.1 widget kinds (grows with the kit; the runtime registry is generated
/// from the same table).
pub const WIDGETS: &[WidgetSpec] = &[
    WidgetSpec {
        name: "Stack",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: true,
    },
    WidgetSpec {
        name: "Spacer",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
    WidgetSpec {
        name: "Text",
        primary_prop: Some("value"),
        interactive: false,
        label_prop: Some("value"),
        allows_children: false,
    },
    WidgetSpec {
        name: "Icon",
        primary_prop: Some("symbol"),
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
    WidgetSpec {
        name: "Image",
        primary_prop: Some("source"),
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
    WidgetSpec {
        name: "Button",
        primary_prop: Some("label"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: true,
    },
    WidgetSpec {
        name: "Card",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: true,
    },
    WidgetSpec {
        name: "TextField",
        primary_prop: Some("value"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: false,
    },
    WidgetSpec {
        name: "Toggle",
        primary_prop: Some("checked"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: false,
    },
    WidgetSpec {
        name: "List",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: true,
    },
    WidgetSpec {
        name: "NativeWidget",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
    // Design-system kit exposure (TASK-0073/0074): each maps 1:1 onto its
    // `userspace/ui/widgets/*` builder in the runtime registry.
    WidgetSpec {
        name: "Badge",
        primary_prop: Some("label"),
        interactive: false,
        label_prop: Some("label"),
        allows_children: false,
    },
    WidgetSpec {
        name: "Chip",
        primary_prop: Some("label"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: false,
    },
    WidgetSpec {
        name: "Avatar",
        primary_prop: Some("initials"),
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
    WidgetSpec {
        name: "Checkbox",
        primary_prop: Some("checked"),
        interactive: true,
        label_prop: Some("label"),
        allows_children: false,
    },
    WidgetSpec {
        name: "Slider",
        primary_prop: Some("value"),
        interactive: true,
        label_prop: None,
        allows_children: false,
    },
    WidgetSpec {
        name: "Spinner",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
    WidgetSpec {
        name: "ProgressBar",
        primary_prop: Some("value"),
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
    WidgetSpec {
        name: "Toast",
        primary_prop: Some("message"),
        interactive: true,
        label_prop: Some("message"),
        allows_children: false,
    },
    WidgetSpec {
        name: "Banner",
        primary_prop: Some("message"),
        interactive: true,
        label_prop: Some("title"),
        allows_children: false,
    },
    WidgetSpec {
        name: "Skeleton",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
    WidgetSpec {
        name: "ListItem",
        primary_prop: Some("title"),
        interactive: true,
        label_prop: Some("title"),
        allows_children: false,
    },
    WidgetSpec {
        name: "Toolbar",
        primary_prop: Some("title"),
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
    WidgetSpec {
        name: "SearchBar",
        primary_prop: Some("value"),
        interactive: true,
        label_prop: Some("placeholder"),
        allows_children: false,
    },
    // Container primitives: material surfaces that host arbitrary children
    // (icons/text/stacks) and take every modifier. `Panel` = the panel-glass
    // surface (Control-Center tiles, window content panels, properties
    // sidebar); `Circle` = a perfectly round container (`size` pins a square
    // box, radius welded to full, content centered) for round buttons,
    // badges and avatar-like elements.
    WidgetSpec {
        name: "Panel",
        primary_prop: None,
        interactive: false,
        label_prop: None,
        allows_children: true,
    },
    WidgetSpec {
        name: "Circle",
        primary_prop: Some("size"),
        interactive: false,
        label_prop: None,
        allows_children: true,
    },
    // Container primitive: a REAL fixed-column grid (`columns: n` = n equal
    // 1fr tracks, row-major fill, `.gap()` = column gap, `rowGap:` = row gap
    // in the same spacing scale). This is the engine's `LayoutNode::Grid` —
    // fully implemented in `nexus-layout` since the node types landed, but
    // unreachable from `.nx` until now; every wrap-flow "grid" in the shell
    // was a silent single overflowing row (`.wrap(true)` is a no-op the
    // engine never reads).
    WidgetSpec {
        name: "Grid",
        primary_prop: Some("columns"),
        interactive: false,
        label_prop: None,
        allows_children: true,
    },
    // Navigation/selection leaves the kit always had but the DSL could not
    // name (settings design handoff). `Select` is the CLOSED trigger only —
    // a glass pill showing the current value plus a chevron; the open option
    // panel is an app-owned `.overlay()`, exactly as the kit crate documents.
    // `Breadcrumbs` renders the whole trail as ONE node, so it shows the path
    // but cannot make individual crumbs tappable — wrap it to navigate.
    WidgetSpec {
        name: "Select",
        primary_prop: Some("value"),
        interactive: true,
        label_prop: Some("placeholder"),
        allows_children: false,
    },
    WidgetSpec {
        name: "Breadcrumbs",
        primary_prop: Some("items"),
        interactive: false,
        label_prop: None,
        allows_children: false,
    },
];

#[must_use]
pub fn widget_spec(name: &str) -> Option<&'static WidgetSpec> {
    WIDGETS.iter().find(|spec| spec.name == name)
}

/// Interaction triggers handlers may bind (`on Tap -> …`). `PageNext`/
/// `PagePrev` are container-scoped like `EndReached`: the host's pager
/// (`.scroll(paged)`) fires them BY NAME when a wheel notch turns the page,
/// so the store's page index stays in sync with the snapped offset.
pub const TRIGGERS: &[&str] = &[
    "Tap",
    "Change",
    "Submit",
    "Focus",
    "Blur",
    "LongPress",
    "EndReached",
    "PageNext",
    "PagePrev",
    // RFC-0086: the compositor's window set moved (an app opened, closed,
    // minimized, restored or took focus) — the shell re-reads the registry
    // so its taskbar/dock markers follow.
    "WindowsChanged",
];

/// The curated **motion token** vocabulary (docs/dev/ui/foundations/animation.md
/// "Recommended v1 scope"). The token argument of `.animate`/`.transition`/
/// `.effect` validates against exactly this closed set — no free-form CSS
/// keyframes, no `--animate-*` vars. This mirrors the runtime SSOT
/// `animation::MotionToken` (same names, same order); the two are kept in
/// lock-step (the runtime resolves the id, the frontend validates the name).
pub const MOTION_TOKENS: &[&str] = &[
    "snappy",
    "smooth",
    "emphasized",
    "fade",
    "slideUp",
    "fadeScale",
    "wiggle",
    "pulse",
    "slideDown",
];

/// Whether `name` is a valid motion token.
#[must_use]
pub fn is_motion_token(name: &str) -> bool {
    MOTION_TOKENS.contains(&name)
}

/// Semantic **color roles** a `.fg`/`.bg`/`.borderColor`/`.bgFade` may name.
/// Mirrors `nexus-dsl-runtime`'s `registry::color_token`, which mirrors
/// `ColorToken` — the three are kept in lock-step (see RFC-0082).
pub const COLOR_TOKENS: &[&str] = &[
    "surface",
    "surfaceVariant",
    "onSurface",
    "onSurfaceVariant",
    "accent",
    "onAccent",
    "border",
    "background",
    "islandBg",
    "primary",
    "onPrimary",
    "danger",
    "onDanger",
    "success",
    "onSuccess",
    "warning",
    "onWarning",
    "info",
    "onInfo",
    "focusRing",
    "shadow",
    "scrim",
    "destructive",
    "onDestructive",
    "onGlass",
    "onGlassMuted",
    "onGlassStrong",
    "glassIcon",
    "glassPlaceholder",
    "glassFocus",
    "glassFill",
    "wallpaperTint",
    "wallpaperVignette",
    "textShadow",
    "textShadowStrong",
    "transparent",
    // Roles the themes have always authored but nothing could name.
    // `divider` is the translucent hairline; `border` is the opaque control
    // outline — they are NOT interchangeable on a glass surface.
    "divider",
    "glassHover",
    "glassActive",
    "toggleOnBg",
    "toggleOffBg",
    "notifDot",
    "sliderTrack",
    "sliderFill",
    "sliderIcon",
];

/// Closed token vocabularies per modifier: `(modifier name, allowed tokens)`.
///
/// A modifier listed here rejects any other token AT COMPILE TIME. Before
/// RFC-0082 an unknown token argument (`.fg(oNSurface)`) type-checked fine and
/// then silently resolved to `None` at runtime — the node just painted the
/// default and the author had no signal at all.
///
/// Only vocabularies that are genuinely closed belong here. Modifiers whose
/// argument is a number in practice (`.padding(4)`, `.width(320)`) are absent
/// on purpose: their `ModArg::Token` spec is historical.
pub const TOKEN_VOCABULARIES: &[(&str, &[&str])] = &[
    ("fg", COLOR_TOKENS),
    ("bg", COLOR_TOKENS),
    ("borderColor", COLOR_TOKENS),
    ("bgFade", COLOR_TOKENS),
    ("textSize", &["xs", "sm", "base", "md", "lg", "xl", "xxl", "xxxl", "display", "hero"]),
    ("fontWeight", &["light", "regular", "medium", "semibold", "bold"]),
    ("textAlign", &["left", "center", "right"]),
    ("leading", &["flat", "tight", "snug", "normal", "relaxed"]),
    ("textShadow", &["none", "soft", "strong"]),
    (
        "material",
        &["opaque", "panel", "card", "subtle", "window", "windowPane", "windowBar", "overlay"],
    ),
    ("rounded", &["sm", "md", "lg", "xl", "xxl", "full"]),
    ("shadow", &["sm", "md", "lg", "xl", "xxl"]),
    ("border", &["thin", "hairline", "medium", "thick"]),
    ("align", &["start", "center", "end", "stretch"]),
    ("justify", &["start", "center", "end", "between", "around"]),
    ("direction", &["row", "column"]),
    ("overflow", &["visible", "hidden"]),
    ("scroll", &["vertical", "horizontal", "paged"]),
];

/// The closed token vocabulary of `name`, if it has one.
#[must_use]
pub fn token_vocabulary(name: &str) -> Option<&'static [&'static str]> {
    TOKEN_VOCABULARIES.iter().find(|(m, _)| *m == name).map(|(_, tokens)| *tokens)
}

/// Read-only device environment fields (docs/dev/dsl/profiles.md) + their
/// value vocabulary where enum-like.
pub const DEVICE_FIELDS: &[(&str, &[&str])] = &[
    ("profile", &["phone", "tablet", "desktop", "tv", "auto", "foldable", "convertible"]),
    ("posture", &["flat", "half_fold", "tent", "book"]),
    ("orientation", &["portrait", "landscape"]),
    ("shellMode", &[]),
    ("sizeClass", &["compact", "regular", "wide"]),
    ("dpiClass", &["low", "normal", "high"]),
    ("input", &["touch", "mouse", "kbd", "remote", "rotary"]),
    // Region axes (RFC-0075/0077 Phase 8b): free-form strings (BCP-47-ish
    // locale tag / keymap layout tag) — no enum vocabulary, like shellMode.
    ("locale", &[]),
    ("keymap", &[]),
    // The active theme mode. Tokens re-theme a tree on their own; this exists
    // for controls that must NAME the mode — an appearance toggle showing the
    // state it is in rather than two buttons for two states.
    ("theme", &["dark", "light"]),
];

#[must_use]
pub fn device_field(name: &str) -> Option<&'static [&'static str]> {
    DEVICE_FIELDS.iter().find(|(field, _)| *field == name).map(|(_, values)| *values)
}

// The `svc.*` service-surface lookup lives in its own module (structure
// ratchet); same public API, re-exported here.
mod svc_surface;
pub use svc_surface::SvcLookup;
#[cfg(feature = "std")]
pub(crate) use svc_surface::{app_surface_guard, set_app_surface};
pub use svc_surface::{svc_method, SvcSig, SVC_SURFACE};
