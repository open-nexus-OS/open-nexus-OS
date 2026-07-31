// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The semantic COLOR vocabulary — one enum, one arm per role in
//! `resources/themes/*.nxtheme.toml`. Split out of `lib.rs` (module-size
//! ratchet): the role list grows with the design system, while the `Tokens`
//! trait and the length/type/motion scales next door do not.
//!
//! Adding a role is a SIX-place lock-step (`token_vocabulary_lockstep`
//! enforces it): here, `theme-tokens/build.rs` ROLES, every
//! `resources/themes/*.nxtheme.toml`, `dsl/core` COLOR_TOKENS, the
//! `dsl/runtime` `color_token` arm, and the test's canonical name.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: userspace/dsl/runtime/tests/token_vocabulary_lockstep.rs

/// Semantic color roles. Add roles here (and to every theme) rather than using
/// raw colors in widgets/shells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorToken {
    /// Base surface (panels, cards, sheets).
    Surface,
    /// A raised/variant surface (hover, selected rows).
    SurfaceVariant,
    /// Primary content (text/icons) on `Surface`.
    OnSurface,
    /// Secondary/dimmed content on `Surface`.
    OnSurfaceVariant,
    /// Accent / primary action.
    Accent,
    /// Content on `Accent`.
    OnAccent,
    /// Hairline borders / separators.
    Border,
    /// Shadow tint.
    Shadow,
    /// Window/desktop background.
    Background,
    /// Brand ink (headings, high-emphasis).
    Primary,
    /// Content on `Primary`.
    OnPrimary,
    /// Destructive / danger action.
    Danger,
    /// Content on `Danger`.
    OnDanger,
    /// Warning status.
    Warning,
    /// Success status.
    Success,
    /// Informational status.
    Info,
    /// Content on `Warning`.
    OnWarning,
    /// Content on `Success`.
    OnSuccess,
    /// Content on `Info`.
    OnInfo,
    /// Focus ring / keyboard focus indicator.
    FocusRing,
    /// The Dynamic-Island pill fill (solid black in both themes).
    IslandBg,
    /// Modal/sheet backdrop dim (handoff `--glass-scrim`).
    Scrim,
    /// Destructive action (handoff role distinct from `Danger`).
    Destructive,
    /// Content on `Destructive`.
    OnDestructive,

    // ---- RFC-0082: content on glass / directly on the wallpaper ----
    // These exist because the lock surface, the dock and the launcher paint
    // over a PHOTOGRAPH, not over `Surface`. `OnSurface` is tuned for a solid
    // panel and disappears on a bright sky.
    /// Primary text on glass or wallpaper.
    OnGlass,
    /// Secondary/dimmed text on glass or wallpaper.
    OnGlassMuted,
    /// High-emphasis text on glass or wallpaper.
    OnGlassStrong,
    /// Icon stroke on glass.
    GlassIcon,
    /// Placeholder text inside a glass field.
    GlassPlaceholder,
    /// Border color a FOCUSED glass control takes (distinct from the solid
    /// `FocusRing` blue — on glass the ring is a tint of the ink).
    GlassFocus,
    /// Flat translucent fill of a control sitting INSIDE glass (a submit
    /// button in a pill). Never its own glass layer — the compositor does not
    /// nest backdrop blur.
    GlassFill,
    /// A RAISED control on glass — stronger than `GlassSubtle`'s tint, weaker
    /// than a solid fill (the calculator's function keys, a pressed chip).
    GlassFillStrong,
    /// The accent as a TINT rather than a fill: an accent-flavoured control
    /// that still reads as glass. Re-derived from the LIVE accent by
    /// [`AccentTokens`], so a user accent moves it too.
    AccentSoft,
    /// Full-bleed wash over the wallpaper that keeps content legible.
    WallpaperTint,
    /// Bottom stop of the wallpaper legibility fade.
    WallpaperVignette,
    /// Soft text-shadow tint (light halo in light mode, drop shadow in dark).
    TextShadow,
    /// Strong text-shadow tint for small labels over busy imagery.
    TextShadowStrong,
    /// Fully transparent — the identity color, used as a gradient stop.
    Transparent,

    // ---- Roles that were AUTHORED in every `.nxtheme.toml` but had no
    // ---- variant here, so nothing above the theme file could reach them.
    /// Hairline separator (handoff `--glass-divider`). Distinct from `Border`:
    /// a divider is a translucent wash that reads on glass, `Border` is the
    /// opaque control outline. Painting hairlines with `Border` is why every
    /// separator in a glass window was a solid `#262626` bar.
    Divider,
    /// Hover wash over an interactive surface (handoff `--glass-hover-bg`).
    GlassHover,
    /// Pressed/active wash over an interactive surface
    /// (handoff `--glass-active-bg`).
    GlassActive,
    /// Track fill of a switch in the ON state (handoff `--glass-toggle-on-bg`).
    ToggleOnBg,
    /// Track fill of a switch in the OFF state.
    ToggleOffBg,
    /// The notification dot (handoff `--glass-notif-dot`, always red).
    NotifDot,
    /// Recessed track of a range slider (handoff `--track`) — dark in BOTH
    /// themes, because the track reads as a groove cut into the surface.
    SliderTrack,
    /// Filled portion of a range slider (handoff `--fill`) — bright in both,
    /// the inverse polarity of its own track.
    SliderFill,
    /// The glyph embedded IN a slider's fill (handoff `.slider .embed`). It
    /// sits on `SliderFill`, so it is dark in both themes like the track.
    SliderIcon,
}
