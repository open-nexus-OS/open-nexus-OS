// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// CONTEXT: Proof panel constants for TASK-0058 windowd integration.
// OWNERS: @ui
// STATUS: Done
// ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md

#[cfg(not(any(nexus_env = "os", target_os = "none")))]
extern crate std;
#[cfg(any(nexus_env = "os", target_os = "none"))]
extern crate alloc;

// Vec is in the std prelude on host, but must be explicitly imported for no_std.
#[cfg(any(nexus_env = "os", target_os = "none"))]
use alloc::vec::Vec;

pub const PANEL_WIDTH: i32 = 610;
pub const PANEL_HEIGHT: i32 = 260;
pub const FILTER_PANEL_WIDTH: i32 = 200;
pub const FILTER_PANEL_HEIGHT: i32 = 260;
pub const PANEL_PADDING: i32 = 24;
pub const PANEL_GAP: i32 = 16;
pub const CARD_WIDTH: i32 = 126;
pub const CARD_HEIGHT: i32 = 82;
pub const CARD_GAP: i32 = 16;
pub const CARD_PADDING: i32 = 14;
pub const CARD_ICON_SIZE: i32 = 24;
pub const ICON_TARGET_SIZE: i32 = 48;

pub const TOKEN_PANEL_BG: &str = "surface";
pub const TOKEN_PANEL_BORDER: &str = "border";
pub const TOKEN_PANEL_TITLE: &str = "fg";
pub const TOKEN_PANEL_SUBTITLE: &str = "accent";
pub const TOKEN_PANEL_MUTED: &str = "mutedFg";
pub const TOKEN_CARD_BG: &str = "surfaceAlt";
pub const TOKEN_CARD_BORDER: &str = "border";
pub const TOKEN_CARD_ACTIVE_BG: &str = "surface";
pub const TOKEN_CARD_LABEL: &str = "fg";
pub const TOKEN_ICON_BG: &str = "accent";
pub const TOKEN_ICON_FG: &str = "accentFg";
pub const TOKEN_HOVER: &str = "accent";
pub const TOKEN_CLICK: &str = "success";
pub const TOKEN_SCROLL: &str = "warning";
pub const TOKEN_KEYBOARD: &str = "focusRing";

#[derive(Clone, Copy)]
pub struct ProofTextSpec {
    pub id: &'static str,
    pub content: &'static str,
    pub font_size: u16,
    pub font_weight: u16,
    pub color_token: &'static str,
}

pub const TITLE_TEXT: ProofTextSpec = ProofTextSpec {
    id: "proof_title",
    content: "Open Nexus OS",
    font_size: 30,
    font_weight: 700,
    color_token: TOKEN_PANEL_TITLE,
};

pub const SUBTITLE_TEXT: ProofTextSpec = ProofTextSpec {
    id: "proof_subtitle",
    content: "DisplayServer v0 - layout engine proof",
    font_size: 18,
    font_weight: 600,
    color_token: TOKEN_PANEL_SUBTITLE,
};

pub const BODY_TEXT: ProofTextSpec = ProofTextSpec {
    id: "proof_body",
    content: "Hover, click, scroll up/down, keyboard press",
    font_size: 16,
    font_weight: 400,
    color_token: TOKEN_PANEL_MUTED,
};

pub const HOVER_LABEL: ProofTextSpec = ProofTextSpec {
    id: "card_hover_label",
    content: "Hover",
    font_size: 16,
    font_weight: 600,
    color_token: TOKEN_CARD_LABEL,
};

pub const CLICK_LABEL: ProofTextSpec = ProofTextSpec {
    id: "card_click_label",
    content: "Click",
    font_size: 16,
    font_weight: 600,
    color_token: TOKEN_CARD_LABEL,
};

pub const SCROLL_LABEL: ProofTextSpec = ProofTextSpec {
    id: "card_scroll_label",
    content: "Scroll",
    font_size: 16,
    font_weight: 600,
    color_token: TOKEN_CARD_LABEL,
};

pub const KEY_LABEL: ProofTextSpec = ProofTextSpec {
    id: "card_key_label",
    content: "Key",
    font_size: 16,
    font_weight: 600,
    color_token: TOKEN_CARD_LABEL,
};

pub const ALL_TEXT_SPECS: &[ProofTextSpec] = &[
    TITLE_TEXT, SUBTITLE_TEXT, BODY_TEXT, HOVER_LABEL, CLICK_LABEL, SCROLL_LABEL, KEY_LABEL,
    FILTER_INPUT_PLACEHOLDER,
    FILTER_INPUT_A,
    FILTER_INPUT_AP,
    FILTER_INPUT_B,
    FILTER_INPUT_C,
    FILTER_WORD_APPLE, FILTER_WORD_APPLICATION, FILTER_WORD_APT, FILTER_WORD_ARROW,
    FILTER_WORD_ASSET, FILTER_WORD_BATCH, FILTER_WORD_BINARY, FILTER_WORD_BLOCK,
    FILTER_WORD_BUFFER, FILTER_WORD_BUILD, FILTER_WORD_CACHE, FILTER_WORD_CLOCK,
    FILTER_WORD_COMPILE, FILTER_WORD_COMPONENT, FILTER_WORD_CONFIG,
];

// ─── Filter input text specs ───
// The visible bootstrap path still uses pre-rendered text assets, so keep the
// proof input vocabulary explicit and deterministic. This maps cleanly to a
// future DSL/pretext text-input spec without hardcoding paint-only heuristics.

macro_rules! filter_input_spec {
    ($id:expr, $text:expr) => {
        ProofTextSpec {
            id: $id,
            content: $text,
            font_size: 14,
            font_weight: 400,
            color_token: TOKEN_PANEL_TITLE,
        }
    };
}

pub const FILTER_INPUT_PLACEHOLDER: ProofTextSpec =
    filter_input_spec!("filter_input_placeholder", "type to filter...");
pub const FILTER_INPUT_A: ProofTextSpec = filter_input_spec!("filter_input_a", "a");
pub const FILTER_INPUT_AP: ProofTextSpec = filter_input_spec!("filter_input_ap", "ap");
pub const FILTER_INPUT_B: ProofTextSpec = filter_input_spec!("filter_input_b", "b");
pub const FILTER_INPUT_C: ProofTextSpec = filter_input_spec!("filter_input_c", "c");

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub(crate) fn filter_input_asset_id(filter_text: &str) -> &'static str {
    match filter_text {
        "" => FILTER_INPUT_PLACEHOLDER.id,
        "a" => FILTER_INPUT_A.id,
        "ap" => FILTER_INPUT_AP.id,
        "b" => FILTER_INPUT_B.id,
        "c" => FILTER_INPUT_C.id,
        _ => FILTER_INPUT_PLACEHOLDER.id,
    }
}

// ─── Filter word text specs ───
// Each filter word gets a pre-rendered text asset so the filter panel can display
// readable text in the OS render path.

macro_rules! filter_word_spec {
    ($id:expr, $word:expr) => {
        ProofTextSpec {
            id: $id,
            content: $word,
            font_size: 14,
            font_weight: 400,
            color_token: TOKEN_PANEL_TITLE,
        }
    };
}

pub const FILTER_WORD_APPLE: ProofTextSpec = filter_word_spec!("filter_apple", "apple");
pub const FILTER_WORD_APPLICATION: ProofTextSpec =
    filter_word_spec!("filter_application", "application");
pub const FILTER_WORD_APT: ProofTextSpec = filter_word_spec!("filter_apt", "apt");
pub const FILTER_WORD_ARROW: ProofTextSpec = filter_word_spec!("filter_arrow", "arrow");
pub const FILTER_WORD_ASSET: ProofTextSpec = filter_word_spec!("filter_asset", "asset");
pub const FILTER_WORD_BATCH: ProofTextSpec = filter_word_spec!("filter_batch", "batch");
pub const FILTER_WORD_BINARY: ProofTextSpec = filter_word_spec!("filter_binary", "binary");
pub const FILTER_WORD_BLOCK: ProofTextSpec = filter_word_spec!("filter_block", "block");
pub const FILTER_WORD_BUFFER: ProofTextSpec = filter_word_spec!("filter_buffer", "buffer");
pub const FILTER_WORD_BUILD: ProofTextSpec = filter_word_spec!("filter_build", "build");
pub const FILTER_WORD_CACHE: ProofTextSpec = filter_word_spec!("filter_cache", "cache");
pub const FILTER_WORD_CLOCK: ProofTextSpec = filter_word_spec!("filter_clock", "clock");
pub const FILTER_WORD_COMPILE: ProofTextSpec = filter_word_spec!("filter_compile", "compile");
pub const FILTER_WORD_COMPONENT: ProofTextSpec =
    filter_word_spec!("filter_component", "component");
pub const FILTER_WORD_CONFIG: ProofTextSpec = filter_word_spec!("filter_config", "config");

// ─── Filter-box word list ───

/// Static word list for the filter-box proof element.
/// Used by `filter_words()` for real-time filtering on each keystroke.
pub const FILTER_WORDS: &[&str] = &[
    "apple",
    "application",
    "apt",
    "arrow",
    "asset",
    "batch",
    "binary",
    "block",
    "buffer",
    "build",
    "cache",
    "clock",
    "compile",
    "component",
    "config",
];

/// Filter the static word list by a case-insensitive prefix.
/// Returns all words that start with `prefix` (ASCII case-insensitive).
/// Pure function — deterministic list spec for the later DSL/pretext lowering.
pub fn filter_words(prefix: &str) -> Vec<&'static str> {
    if prefix.is_empty() {
        return FILTER_WORDS.to_vec();
    }
    let lower = prefix.to_ascii_lowercase();
    FILTER_WORDS
        .iter()
        .filter(|word| word.to_ascii_lowercase().starts_with(&lower))
        .copied()
        .collect()
}
