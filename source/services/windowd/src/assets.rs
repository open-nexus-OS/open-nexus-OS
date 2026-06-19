// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

mod generated {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/windowd_generated_assets.rs"));
}

use nexus_layout_types::Rgba8;

/// Embedded Mocu cursor SVG, normalized from `resources/cursors/mocu/src/svg/default.svg`.
pub const CURSOR_LEFT_PTR_SVG: &str = generated::MOCU_CURSOR_LEFT_PTR_SVG;
pub const CURSOR_LEFT_PTR_BGRA: &[u8] = generated::MOCU_CURSOR_BGRA;
pub const CURSOR_LEFT_PTR_WIDTH: u32 = generated::MOCU_CURSOR_WIDTH;
pub const CURSOR_LEFT_PTR_HEIGHT: u32 = generated::MOCU_CURSOR_HEIGHT;
pub const CURSOR_HOTSPOT_X: i32 = generated::MOCU_CURSOR_HOTSPOT_X;
pub const CURSOR_HOTSPOT_Y: i32 = generated::MOCU_CURSOR_HOTSPOT_Y;

/// Real Lucide icon (house), rendered via the nexus-svg HiDPI pipeline at build
/// time. Uploaded to gpud once and composited as a GPU sprite layer on the virgl
/// scanout — the "real icon layer" (TASK #61).
pub const SHELL_ICON_BGRA: &[u8] = generated::SHELL_ICON_BGRA;
pub const SHELL_ICON_WIDTH: u32 = generated::SHELL_ICON_WIDTH;
pub const SHELL_ICON_HEIGHT: u32 = generated::SHELL_ICON_HEIGHT;
/// On-screen (logical) size the icon composites at; the texture above is 2× this
/// (supersampled) and GPU-downscaled to it.
pub const SHELL_ICON_LOGICAL: u32 = generated::SHELL_ICON_LOGICAL;

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
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub const GLASS_TINT: Rgba8 = rgba8(generated::GLASS_TINT_RGBA);
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub const GLASS_EDGE: Rgba8 = rgba8(generated::GLASS_EDGE_RGBA);

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
        "filter_input_placeholder" => ProofTextAsset {
            width: generated::FILTER_INPUT_PLACEHOLDER_WIDTH,
            height: generated::FILTER_INPUT_PLACEHOLDER_HEIGHT,
            bgra: generated::FILTER_INPUT_PLACEHOLDER_BGRA,
        },
        "filter_input_a" => ProofTextAsset {
            width: generated::FILTER_INPUT_A_WIDTH,
            height: generated::FILTER_INPUT_A_HEIGHT,
            bgra: generated::FILTER_INPUT_A_BGRA,
        },
        "filter_input_ap" => ProofTextAsset {
            width: generated::FILTER_INPUT_AP_WIDTH,
            height: generated::FILTER_INPUT_AP_HEIGHT,
            bgra: generated::FILTER_INPUT_AP_BGRA,
        },
        "filter_input_b" => ProofTextAsset {
            width: generated::FILTER_INPUT_B_WIDTH,
            height: generated::FILTER_INPUT_B_HEIGHT,
            bgra: generated::FILTER_INPUT_B_BGRA,
        },
        "filter_input_c" => ProofTextAsset {
            width: generated::FILTER_INPUT_C_WIDTH,
            height: generated::FILTER_INPUT_C_HEIGHT,
            bgra: generated::FILTER_INPUT_C_BGRA,
        },
        "filter_apple" => ProofTextAsset {
            width: generated::FILTER_APPLE_WIDTH,
            height: generated::FILTER_APPLE_HEIGHT,
            bgra: generated::FILTER_APPLE_BGRA,
        },
        "filter_application" => ProofTextAsset {
            width: generated::FILTER_APPLICATION_WIDTH,
            height: generated::FILTER_APPLICATION_HEIGHT,
            bgra: generated::FILTER_APPLICATION_BGRA,
        },
        "filter_apt" => ProofTextAsset {
            width: generated::FILTER_APT_WIDTH,
            height: generated::FILTER_APT_HEIGHT,
            bgra: generated::FILTER_APT_BGRA,
        },
        "filter_arrow" => ProofTextAsset {
            width: generated::FILTER_ARROW_WIDTH,
            height: generated::FILTER_ARROW_HEIGHT,
            bgra: generated::FILTER_ARROW_BGRA,
        },
        "filter_asset" => ProofTextAsset {
            width: generated::FILTER_ASSET_WIDTH,
            height: generated::FILTER_ASSET_HEIGHT,
            bgra: generated::FILTER_ASSET_BGRA,
        },
        "filter_batch" => ProofTextAsset {
            width: generated::FILTER_BATCH_WIDTH,
            height: generated::FILTER_BATCH_HEIGHT,
            bgra: generated::FILTER_BATCH_BGRA,
        },
        "filter_binary" => ProofTextAsset {
            width: generated::FILTER_BINARY_WIDTH,
            height: generated::FILTER_BINARY_HEIGHT,
            bgra: generated::FILTER_BINARY_BGRA,
        },
        "filter_block" => ProofTextAsset {
            width: generated::FILTER_BLOCK_WIDTH,
            height: generated::FILTER_BLOCK_HEIGHT,
            bgra: generated::FILTER_BLOCK_BGRA,
        },
        "filter_buffer" => ProofTextAsset {
            width: generated::FILTER_BUFFER_WIDTH,
            height: generated::FILTER_BUFFER_HEIGHT,
            bgra: generated::FILTER_BUFFER_BGRA,
        },
        "filter_build" => ProofTextAsset {
            width: generated::FILTER_BUILD_WIDTH,
            height: generated::FILTER_BUILD_HEIGHT,
            bgra: generated::FILTER_BUILD_BGRA,
        },
        "filter_cache" => ProofTextAsset {
            width: generated::FILTER_CACHE_WIDTH,
            height: generated::FILTER_CACHE_HEIGHT,
            bgra: generated::FILTER_CACHE_BGRA,
        },
        "filter_clock" => ProofTextAsset {
            width: generated::FILTER_CLOCK_WIDTH,
            height: generated::FILTER_CLOCK_HEIGHT,
            bgra: generated::FILTER_CLOCK_BGRA,
        },
        "filter_compile" => ProofTextAsset {
            width: generated::FILTER_COMPILE_WIDTH,
            height: generated::FILTER_COMPILE_HEIGHT,
            bgra: generated::FILTER_COMPILE_BGRA,
        },
        "filter_component" => ProofTextAsset {
            width: generated::FILTER_COMPONENT_WIDTH,
            height: generated::FILTER_COMPONENT_HEIGHT,
            bgra: generated::FILTER_COMPONENT_BGRA,
        },
        "filter_config" => ProofTextAsset {
            width: generated::FILTER_CONFIG_WIDTH,
            height: generated::FILTER_CONFIG_HEIGHT,
            bgra: generated::FILTER_CONFIG_BGRA,
        },
        _ => return None,
    };
    Some(asset)
}

const fn rgba8(value: [u8; 4]) -> Rgba8 {
    Rgba8::new(value[0], value[1], value[2], value[3])
}

#[cfg(test)]
mod tests {
    #[test]
    fn cursor_asset_is_generated_bgra() {
        assert_eq!(
            super::CURSOR_LEFT_PTR_BGRA.len(),
            (super::CURSOR_LEFT_PTR_WIDTH * super::CURSOR_LEFT_PTR_HEIGHT * 4) as usize
        );
    }
}
