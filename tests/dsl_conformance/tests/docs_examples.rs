// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The DSL docs cannot drift from the language: `docs/dev/dsl/` is outside the docs gate
//! (`nx ledger lint` skips `docs/dev/`), so these two checks are its gate.
//!
//! 1. Every example compiles. A fenced block tagged `nx` is a whole program; one tagged
//!    `nx-view` is a view fragment, checked inside `Page DocExample { Stack { … } }`. Any other
//!    tag (`text`, `ebnf`, `bash`, …) is not checked — pseudo-code says so with its tag.
//! 2. Every widget of the catalog (`WIDGETS`) has its reference entry — a `### \`Name\``
//!    heading in `widgets.md` — and every live modifier (`MODIFIERS`, minus the retired ones)
//!    appears in `modifiers.md` as `` `.name( `` or `` `.name` ``.
//!
//! Before this gate, 31 of 35 `nx` examples in the docs no longer compiled and the widgets had
//! no reference page at all.

use nexus_dsl_core::registry::{retired_modifier, MODIFIERS, WIDGETS};
use std::path::{Path, PathBuf};

fn docs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/dev/dsl")
}

/// `(file, block index, info tag, body, first line number)` of every fenced block.
fn blocks(file: &Path) -> Vec<(String, usize, String, String, usize)> {
    let text = std::fs::read_to_string(file).expect("doc readable");
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_owned();
    let mut out = Vec::new();
    let mut lines = text.lines().enumerate();
    let mut index = 0;
    while let Some((n, line)) = lines.next() {
        let Some(tag) = line.trim_start().strip_prefix("```") else { continue };
        let tag = tag.trim().to_owned();
        let mut body = String::new();
        for (_, inner) in lines.by_ref() {
            if inner.trim_start().starts_with("```") {
                break;
            }
            body.push_str(inner);
            body.push('\n');
        }
        out.push((name.clone(), index, tag, body, n + 2));
        index += 1;
    }
    out
}

fn compile(src: &str) -> Result<(), String> {
    let file = nexus_dsl_core::parse_file(src).map_err(|e| format!("{e:?}"))?;
    let (model, diags) = nexus_dsl_core::check_file(&file);
    if nexus_dsl_core::has_errors(&diags) {
        let first =
            diags.first().map_or_else(String::new, |d| format!("{:?}: {}", d.code, d.message));
        return Err(first);
    }
    nexus_dsl_core::lower_file(&file, &model, src).map(|_| ()).map_err(|e| format!("{e:?}"))
}

fn md_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(docs_dir())
        .expect("docs/dev/dsl")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    files.sort();
    files
}

#[test]
fn every_nx_example_in_the_dsl_docs_compiles() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for file in md_files() {
        for (name, index, tag, body, line) in blocks(&file) {
            let src = match tag.as_str() {
                "nx" => body,
                "nx-view" => format!("Page DocExample {{\n    Stack {{\n{body}    }}\n}}\n"),
                _ => continue,
            };
            checked += 1;
            if let Err(e) = compile(&src) {
                failures.push(format!("{name}:{line} (block {index}, `{tag}`): {e}"));
            }
        }
    }
    assert!(checked >= 20, "found only {checked} examples — the extractor is broken");
    assert!(
        failures.is_empty(),
        "{} of {checked} DSL doc examples do not compile:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

#[test]
fn every_widget_and_modifier_has_its_reference_entry() {
    let widgets = std::fs::read_to_string(docs_dir().join("widgets.md")).unwrap_or_default();
    let missing_widgets: Vec<&str> = WIDGETS
        .iter()
        .map(|w| w.name)
        .filter(|n| !widgets.contains(&format!("### `{n}`")))
        .collect();
    let modifiers = std::fs::read_to_string(docs_dir().join("modifiers.md")).expect("modifiers.md");
    let missing_modifiers: Vec<&str> = MODIFIERS
        .iter()
        .map(|m| m.name)
        .filter(|n| retired_modifier(n).is_none())
        .filter(|n| {
            !modifiers.contains(&format!("`.{n}(")) && !modifiers.contains(&format!("`.{n}`"))
        })
        .collect();
    assert!(
        missing_widgets.is_empty() && missing_modifiers.is_empty(),
        "undocumented — widgets (a `### `Name`` heading in docs/dev/dsl/widgets.md): \
         {missing_widgets:?}; modifiers (in docs/dev/dsl/modifiers.md): {missing_modifiers:?}"
    );
}
