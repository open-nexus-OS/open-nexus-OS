// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host render environment — the static theme-token sets (base/
//! dark/light + accent snapshots) and the pushed-profile → `FixtureEnv`
//! mapping every mount/dispatch site shares (moved out of `main.rs`,
//! structure gate).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: exercised by every app render; theme/profile switching is
//! proven by the windowd push markers in QEMU boots.

use super::wire;

// Static theme token sets (ZSTs) → a runtime-selectable `&'static dyn Tokens`.
static BASE_TOKENS: nexus_dsl_runtime::theme_tokens::BaseTokens =
    nexus_dsl_runtime::theme_tokens::BaseTokens;
static DARK_TOKENS: nexus_dsl_runtime::theme_tokens::DarkTokens =
    nexus_dsl_runtime::theme_tokens::DarkTokens;
static LIGHT_TOKENS: nexus_dsl_runtime::theme_tokens::LightTokens =
    nexus_dsl_runtime::theme_tokens::LightTokens;

/// One accent-overridden snapshot per palette entry × mode (alloc-free —
/// `tokens_for` returns `&'static`). Index = palette index − 1.
const fn accented(
    base: &'static (dyn nexus_dsl_runtime::theme_tokens::Tokens + Sync),
    idx: u8,
    dark: bool,
) -> nexus_dsl_runtime::theme_tokens::AccentTokens {
    let accent = match nexus_dsl_runtime::theme_tokens::accent_color(idx, dark) {
        Some(c) => c,
        // Unreachable for 1..=5; keep the built-in blue as a safe value.
        None => nexus_layout_types::Rgba8::new(59, 130, 246, 255),
    };
    nexus_dsl_runtime::theme_tokens::AccentTokens { base, accent }
}
static ACCENTED_DARK: [nexus_dsl_runtime::theme_tokens::AccentTokens; 5] = [
    accented(&DARK_TOKENS, 1, true),
    accented(&DARK_TOKENS, 2, true),
    accented(&DARK_TOKENS, 3, true),
    accented(&DARK_TOKENS, 4, true),
    accented(&DARK_TOKENS, 5, true),
];
static ACCENTED_LIGHT: [nexus_dsl_runtime::theme_tokens::AccentTokens; 5] = [
    accented(&LIGHT_TOKENS, 1, false),
    accented(&LIGHT_TOKENS, 2, false),
    accented(&LIGHT_TOKENS, 3, false),
    accented(&LIGHT_TOKENS, 4, false),
    accented(&LIGHT_TOKENS, 5, false),
];

/// The token set for a PACKED wire theme byte (`mode | accent << 4`) — the
/// app renders with the SAME tokens the compositor pushed (dark desktop ⇒
/// dark app; user accent ⇒ accented widgets). The packed byte flows
/// through `theme_mode` unchanged, so an accent switch re-mounts via the
/// same `mode != theme_mode` path as a light/dark toggle.
pub(crate) fn tokens_for(packed: u8) -> &'static dyn nexus_dsl_runtime::theme_tokens::Tokens {
    let (mode, accent) = wire::unpack_theme(packed);
    let accent_slot = (accent as usize).checked_sub(1);
    match (mode, accent_slot) {
        (wire::THEME_DARK, Some(i)) if i < ACCENTED_DARK.len() => &ACCENTED_DARK[i],
        (wire::THEME_LIGHT, Some(i)) if i < ACCENTED_LIGHT.len() => &ACCENTED_LIGHT[i],
        (wire::THEME_DARK, _) => &DARK_TOKENS,
        (wire::THEME_LIGHT, _) => &LIGHT_TOKENS,
        _ => &BASE_TOKENS,
    }
}

/// The width class of the TOUCH axis (design_handoff_launcher: mode ⟂
/// width — `desktopMode` is an explicit toggle, width only picks between
/// the touch layouts). Mobile-first breakpoints, `device.sizeClass`:
/// compact = phone (<640), regular = tablet portrait (<1024), wide =
/// landscape (≥1024).
pub(crate) fn size_class_for(w: u32) -> &'static str {
    if w < 640 {
        "compact"
    } else if w < 1024 {
        "regular"
    } else {
        "wide"
    }
}

/// The device environment for a pushed shell profile — what the DSL's
/// `device.profile` reads, so `ui/platform/<profile>/` override arms
/// select to the environment's windowing policy. Touch profiles derive
/// `device.sizeClass` from the REAL surface width (the handoff's `vw`
/// axis); desktop mode ignores width (one taskbar layout).
pub(crate) fn device_for(
    profile: u8,
    w: u32,
    locale: &str,
    keymap: &str,
) -> nexus_dsl_runtime::FixtureEnv {
    use nexus_dsl_runtime::FixtureEnv;
    let mut env = match profile {
        wire::PROFILE_DESKTOP => FixtureEnv::desktop(),
        profile => {
            let mut env = if profile == wire::PROFILE_PHONE {
                FixtureEnv::phone("portrait")
            } else {
                // Our display is landscape 1280×800 (touch-landscape).
                FixtureEnv::tablet("landscape")
            };
            env.size_class = size_class_for(w);
            env
        }
    };
    // Region axes (RFC-0075 Phase 8b): runtime-varying, from the windowd
    // region push — `if device.locale/keymap` arms re-select on reemit.
    env.locale = alloc::string::String::from(locale);
    env.keymap = alloc::string::String::from(keymap);
    env
}
