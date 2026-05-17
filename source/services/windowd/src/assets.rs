// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

mod generated {
    include!(concat!(env!("OUT_DIR"), "/windowd_generated_assets.rs"));
}

use nexus_layout_types::Rgba8;

/// Embedded Mocu cursor SVG, normalized from `resources/cursors/mocu/src/svg/default.svg`.
pub const CURSOR_LEFT_PTR_SVG: &str = generated::MOCU_CURSOR_LEFT_PTR_SVG;
pub const CURSOR_HOTSPOT_X: i32 = generated::MOCU_CURSOR_HOTSPOT_X;
pub const CURSOR_HOTSPOT_Y: i32 = generated::MOCU_CURSOR_HOTSPOT_Y;

pub struct ProofTextAsset {
    pub width: u32,
    pub height: u32,
    pub bgra: &'static [u8],
}

pub const PROOF_PANEL_BG: Rgba8 = rgba8(generated::PROOF_PANEL_BG_RGBA);
pub const PROOF_PANEL_BORDER: Rgba8 = rgba8(generated::PROOF_PANEL_BORDER_RGBA);
pub const PROOF_PANEL_TITLE: Rgba8 = rgba8(generated::PROOF_PANEL_TITLE_RGBA);
pub const PROOF_PANEL_SUBTITLE: Rgba8 = rgba8(generated::PROOF_PANEL_SUBTITLE_RGBA);
pub const PROOF_PANEL_MUTED: Rgba8 = rgba8(generated::PROOF_PANEL_MUTED_RGBA);
pub const PROOF_CARD_BG: Rgba8 = rgba8(generated::PROOF_CARD_BG_RGBA);
pub const PROOF_CARD_ACTIVE_BG: Rgba8 = rgba8(generated::PROOF_CARD_ACTIVE_BG_RGBA);
pub const PROOF_CARD_BORDER: Rgba8 = rgba8(generated::PROOF_CARD_BORDER_RGBA);
pub const PROOF_CARD_LABEL: Rgba8 = rgba8(generated::PROOF_CARD_LABEL_RGBA);
pub const PROOF_ICON_BG: Rgba8 = rgba8(generated::PROOF_ICON_BG_RGBA);
pub const PROOF_ICON_FG: Rgba8 = rgba8(generated::PROOF_ICON_FG_RGBA);
pub const PROOF_HOVER: Rgba8 = rgba8(generated::PROOF_HOVER_RGBA);
pub const PROOF_CLICK: Rgba8 = rgba8(generated::PROOF_CLICK_RGBA);
pub const PROOF_SCROLL: Rgba8 = rgba8(generated::PROOF_SCROLL_RGBA);
pub const PROOF_KEYBOARD: Rgba8 = rgba8(generated::PROOF_KEYBOARD_RGBA);

pub fn proof_text_asset(id: &str) -> Option<ProofTextAsset> {
    let asset = match id {
        "proof_title" => ProofTextAsset {
            width: generated::PROOF_TITLE_WIDTH,
            height: generated::PROOF_TITLE_HEIGHT,
            bgra: generated::PROOF_TITLE_BGRA,
        },
        "proof_subtitle" => ProofTextAsset {
            width: generated::PROOF_SUBTITLE_WIDTH,
            height: generated::PROOF_SUBTITLE_HEIGHT,
            bgra: generated::PROOF_SUBTITLE_BGRA,
        },
        "proof_body" => ProofTextAsset {
            width: generated::PROOF_BODY_WIDTH,
            height: generated::PROOF_BODY_HEIGHT,
            bgra: generated::PROOF_BODY_BGRA,
        },
        "card_hover_label" => ProofTextAsset {
            width: generated::CARD_HOVER_LABEL_WIDTH,
            height: generated::CARD_HOVER_LABEL_HEIGHT,
            bgra: generated::CARD_HOVER_LABEL_BGRA,
        },
        "card_click_label" => ProofTextAsset {
            width: generated::CARD_CLICK_LABEL_WIDTH,
            height: generated::CARD_CLICK_LABEL_HEIGHT,
            bgra: generated::CARD_CLICK_LABEL_BGRA,
        },
        "card_scroll_label" => ProofTextAsset {
            width: generated::CARD_SCROLL_LABEL_WIDTH,
            height: generated::CARD_SCROLL_LABEL_HEIGHT,
            bgra: generated::CARD_SCROLL_LABEL_BGRA,
        },
        "card_key_label" => ProofTextAsset {
            width: generated::CARD_KEY_LABEL_WIDTH,
            height: generated::CARD_KEY_LABEL_HEIGHT,
            bgra: generated::CARD_KEY_LABEL_BGRA,
        },
        _ => return None,
    };
    Some(asset)
}

const fn rgba8(value: [u8; 4]) -> Rgba8 {
    Rgba8::new(value[0], value[1], value[2], value[3])
}
