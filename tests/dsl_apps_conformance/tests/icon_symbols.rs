// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Icon symbols are the one part of a `.nx` page the compiler CANNOT check.
//
// `Icon { symbol: … }` takes a `Str`, and every window-kit component forwards
// one as a plain prop string, so a misspelled or unmapped name type-checks,
// lowers, mounts and lays out — and then `registry/widgets.rs` paints its
// honest grey placeholder box, which reads as "the designer forgot an icon"
// rather than "the name is wrong". Four symbols shipped that way
// (`square.grid.2x2`, `arrow.clockwise`, `arrow.uturn.backward`, `calendar`).
//
// This test closes that hole from the only side that can see it: the shipped
// source. It scans every `.nx` file for the shapes that carry a symbol name and
// asserts each one resolves against the theme-linked set
// (`[icons.symbols]` in `resources/themes/base.nxtheme.toml`) or the legacy
// built-in fallbacks. It is a STRING scan on purpose — the value never survives
// as a literal past lowering, so there is nothing later to inspect.

use std::path::{Path, PathBuf};

/// Prop names whose value is an icon symbol. `symbol` is the `Icon` widget's
/// own prop; the rest are the window-kit forwarding props (`WinTopBar` tools +
/// nav cluster, `WinSideItem`/`WinMenuItem`/`WinPropRow`/`WinAction*` icons).
/// Adding a component prop that feeds an `Icon` means adding it here — that is
/// the maintenance cost of checking a name the type system cannot.
const SYMBOL_PROPS: &[&str] =
    &["symbol", "icon", "tool1", "tool2", "tool3", "navBack", "navFwd", "navRefresh"];

/// The camelCase built-ins `registry/widgets.rs` still resolves after the
/// theme-linked set misses (`Symbol::Plus` … `Symbol::ChevronUp`).
const LEGACY_FALLBACKS: &[&str] =
    &["plus", "minus", "close", "star", "chevronRight", "chevronLeft", "chevronDown", "chevronUp"];

fn apps_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../userspace/apps")
}

fn nx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            nx_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "nx") {
            out.push(path);
        }
    }
}

/// `<prop>: "<value>"` occurrences of the symbol-carrying props in one line.
/// An empty value is the documented "hide this slot" sentinel (`WinTopBar`
/// `tool2: ""`), not a symbol.
fn symbols_in_line(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    for prop in SYMBOL_PROPS {
        let mut from = 0;
        while let Some(at) = line[from..].find(prop) {
            let start = from + at;
            from = start + prop.len();
            // The match must be a whole identifier followed by `:` and a
            // string literal — `tool1Active: $state.x` and `iconSvg` must not
            // count (`tool1Active` fails the `:`, `iconSvg` the char check).
            let before_ok = start == 0
                || !line.as_bytes()[start - 1].is_ascii_alphanumeric()
                    && line.as_bytes()[start - 1] != b'_';
            if !before_ok {
                continue;
            }
            let rest = line[from..].trim_start();
            let Some(rest) = rest.strip_prefix(':') else { continue };
            let rest = rest.trim_start();
            let Some(rest) = rest.strip_prefix('"') else { continue };
            let Some(end) = rest.find('"') else { continue };
            if !rest[..end].is_empty() {
                found.push(rest[..end].to_string());
            }
        }
    }
    found
}

fn resolves(symbol: &str) -> bool {
    nexus_widget_icon::lucide_symbol_named(symbol).is_some()
        || LEGACY_FALLBACKS.contains(&symbol)
        // `Image { source: "mime:…" }` / an app id is a RASTER sprite, not a
        // vector symbol — a different widget arm, out of scope here.
        || symbol.starts_with("mime:")
}

/// Every icon symbol every shipped app names must resolve. A failure lists the
/// file, the line and the name, because the whole point is that the running
/// system cannot tell you which one it dropped.
#[test]
fn every_shipped_icon_symbol_resolves() {
    let mut files = Vec::new();
    nx_files(&apps_dir(), &mut files);
    assert!(!files.is_empty(), "no .nx files found under {}", apps_dir().display());

    let mut broken: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for symbol in symbols_in_line(line) {
                checked += 1;
                if !resolves(&symbol) {
                    broken.push(format!("{}:{}: `{symbol}`", file.display(), i + 1));
                }
            }
        }
    }

    assert!(
        checked > 50,
        "scan found only {checked} symbols — the extractor is broken, not the apps"
    );
    assert!(
        broken.is_empty(),
        "{} icon symbol(s) resolve to the grey placeholder box — add them to \
         `[icons.symbols]` in resources/themes/base.nxtheme.toml:\n  {}",
        broken.len(),
        broken.join("\n  ")
    );
}

/// The `Image { source: "mime:…" }` sprites are a separate path with its own
/// SSOT; this test does not police them, and says so rather than pretending
/// the scan above covers every glyph on screen.
#[test]
fn mime_sprites_are_out_of_scope() {
    assert!(resolves("mime:inode-directory"));
    assert!(!resolves("definitely.not.a.symbol"));
}
