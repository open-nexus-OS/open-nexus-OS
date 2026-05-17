// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Proof panel constants for TASK-0058 windowd integration.
//! OWNERS: @ui
//! STATUS: Done
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md
// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

pub const PANEL_WIDTH: i32 = 610;
pub const PANEL_HEIGHT: i32 = 260;
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

pub const ALL_TEXT_SPECS: &[ProofTextSpec] =
    &[TITLE_TEXT, SUBTITLE_TEXT, BODY_TEXT, HOVER_LABEL, CLICK_LABEL, SCROLL_LABEL, KEY_LABEL];
