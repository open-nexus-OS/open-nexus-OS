// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The canonical form of the app's window intent (`Window { … }`): its frame fields always, its
//! size fields (RFC-0117) only when the app declares them. Split from `mod.rs` (module-size ratchet).

use super::indent_to;
use crate::ast::{WindowDecl, WindowLevel, WindowMode, WindowStyle};
use alloc::string::String;

pub(super) fn window_to(out: &mut String, window: &WindowDecl) {
    let field = |out: &mut String, key: &str, value: &str| {
        indent_to(out, 1);
        out.push_str(key);
        out.push_str(": ");
        out.push_str(value);
        out.push_str(",\n");
    };
    out.push_str("Window {\n");
    field(
        out,
        "style",
        match window.style {
            WindowStyle::Titlebar => "titlebar",
            WindowStyle::HiddenTitlebar => "hiddenTitlebar",
            WindowStyle::Plain => "plain",
        },
    );
    field(
        out,
        "mode",
        match window.mode {
            WindowMode::Auto => "auto",
            WindowMode::Freeform => "freeform",
            WindowMode::Fullscreen => "fullscreen",
        },
    );
    field(
        out,
        "level",
        match window.level {
            WindowLevel::Normal => "normal",
            WindowLevel::Desktop => "desktop",
            WindowLevel::Overlay => "overlay",
        },
    );
    field(out, "resizable", if window.resizable { "true" } else { "false" });
    for (key, value) in [
        ("width", window.width),
        ("height", window.height),
        ("minWidth", window.min_width),
        ("minHeight", window.min_height),
    ] {
        if let Some(px) = value {
            field(out, key, &alloc::format!("{px}"));
        }
    }
    out.push_str("}\n");
}
