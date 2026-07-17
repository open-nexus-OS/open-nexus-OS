// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! ⚠ CLEANUP-MAP (docs/dev/ui/windowd-cleanup-map.md): MOVE → Shell-/Widget-Assets (UI-Assets gehören der UI).
//! DO NOT EXTEND — new capability belongs at the target, not here.
//

mod generated {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/windowd_generated_assets.rs"));
}

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
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const SHELL_ICON_BGRA: &[u8] = generated::SHELL_ICON_BGRA;
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const SHELL_ICON_WIDTH: u32 = generated::SHELL_ICON_WIDTH;
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const SHELL_ICON_HEIGHT: u32 = generated::SHELL_ICON_HEIGHT;
/// On-screen (logical) size the icon composites at; the texture above is 2× this
/// (supersampled) and GPU-downscaled to it.
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const SHELL_ICON_LOGICAL: u32 = generated::SHELL_ICON_LOGICAL;

/// Resize pointer shapes (TASK-0070 Phase 3): vendored cursor-theme
/// `ew`/`ns`/`nesw`/`nwse` variants, 32×32 like the default pointer,
/// hotspot = center (16,16).
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const CURSOR_RESIZE_EW_BGRA: &[u8] = generated::CURSOR_RESIZE_EW_BGRA;
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const CURSOR_RESIZE_NS_BGRA: &[u8] = generated::CURSOR_RESIZE_NS_BGRA;
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const CURSOR_RESIZE_NESW_BGRA: &[u8] = generated::CURSOR_RESIZE_NESW_BGRA;
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const CURSOR_RESIZE_NWSE_BGRA: &[u8] = generated::CURSOR_RESIZE_NWSE_BGRA;
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const CURSOR_RESIZE_HOTSPOT: i32 = 16;
/// Loading-ring wait-cursor frames (animated wait cursor): 32×32 premultiplied
/// BGRA, hotspot = center, one sprite per ring rotation step.
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const CURSOR_RING_FRAMES: [&[u8]; 8] = generated::CURSOR_RING_FRAMES;

/// Dock icon for minimized windows (Lucide `search`). Consumed by the os-lite
/// dock rasterizer (`compositor::runtime::wm`).
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const DOCK_SEARCH_ICON_BGRA: &[u8] = generated::DOCK_SEARCH_ICON_BGRA;
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const DOCK_SEARCH_ICON_DIM: u32 = generated::DOCK_SEARCH_ICON_DIM;

pub struct ProofTextAsset {
    pub width: u32,
    pub height: u32,
    pub bgra: &'static [u8],
}


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

/// Runtime glyph atlases (TASK-0070 Phase 6): A8 coverage + metrics + sparse
/// kerning of the vendored UI face, baked by build.rs at the two shell text
/// sizes. Consumed exclusively by `crate::text`. `FONT_FAMILY` is the
/// manifest-driven default behind the prepared `ui.font.family` settings key.
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const FONT_FAMILY: &str = generated::FONT_FAMILY;

/// Baked dual theme snapshots (TASK-0072 Phase 9): the same token vocabulary in
/// BGRA for both qualifiers. The compositor swaps between them on a light/dark
/// switch — see [`crate::theme`].
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const THEME_DARK: crate::theme::ThemeTokens = generated::THEME_DARK;
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub const THEME_LIGHT: crate::theme::ThemeTokens = generated::THEME_LIGHT;

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

/// Blend one row of a straight-alpha BGRA icon into a BGRA row buffer,
/// optionally tinting the glyph. Relocated from the deleted legacy
/// `desktop_layer` (cleanup-map DELETE): the dock still rasterizes minimized
/// icons until it moves to the DSL shell (MOVE column).
#[cfg_attr(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")), allow(dead_code))]
pub(crate) fn blend_icon_row(
    row: &mut [u8],
    dst_x: u32,
    icon: &[u8],
    dim: u32,
    icon_row: u32,
    alpha_mul: u8,
    tint: Option<[u8; 3]>,
) {
    if icon_row >= dim {
        return;
    }
    let rp = (row.len() / 4) as u32;
    let src_off = (icon_row * dim) as usize * 4;
    for ix in 0..dim {
        let px = dst_x + ix;
        if px >= rp {
            break;
        }
        let s = src_off + ix as usize * 4;
        if s + 4 > icon.len() {
            break;
        }
        let a = u32::from(icon[s + 3]) * u32::from(alpha_mul) / 255;
        if a == 0 {
            continue;
        }
        let inv = 255 - a;
        let d = px as usize * 4;
        for ch in 0..3 {
            let src = match tint {
                Some(t) => u32::from(t[ch]),
                None => u32::from(icon[s + ch]),
            };
            row[d + ch] = ((src * a + u32::from(row[d + ch]) * inv) / 255) as u8;
        }
    }
}
