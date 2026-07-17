// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nx-dsl i18n` verbs — `extract` (program keys → authoring JSON,
//! preserving existing translations) and `compile` (JSON → the deterministic
//! `NXC1` binary catalog the runtime loads via `Catalog::from_binary`).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/dsl_v0_2_host (extract/compile round-trip)

use std::path::Path;
use std::process::ExitCode;

/// `i18n extract <appdir|file.nx> -o i18n/en.json` /
/// `i18n compile <file.json> -o <file.nxc>`
pub fn cmd_i18n(args: &[String]) -> ExitCode {
    let sub = args.first().map(String::as_str).unwrap_or("");
    let out_path = args.iter().position(|a| a == "-o").and_then(|i| args.get(i + 1)).cloned();
    let inputs: Vec<&String> = args[1.min(args.len())..]
        .iter()
        .filter(|a| *a != "-o" && Some(a.as_str()) != out_path.as_deref())
        .collect();
    match (sub, inputs.first(), out_path) {
        ("extract", Some(input), Some(out)) => extract(input, &out),
        ("compile", Some(input), Some(out)) => compile(input, &out),
        _ => {
            eprintln!(
                "usage: nx-dsl i18n extract <appdir> -o i18n/en.json\n\
                 \x20      nx-dsl i18n compile <catalog.json> -o <catalog.nxc>"
            );
            ExitCode::from(2)
        }
    }
}

fn extract(input: &str, out: &str) -> ExitCode {
    let (file, canonical, _) = match crate::load_input(input) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let (model, diags) = nexus_dsl_core::check_file(&file);
    if nexus_dsl_core::has_errors(&diags) {
        eprintln!("nx-dsl i18n extract: the program has errors; fix them first");
        return ExitCode::from(1);
    }
    // Keys come from the lowered IR — the same canonical table the runtime
    // resolves, so extract can never drift from execution.
    let Ok(lowered) = nexus_dsl_core::lower_file(&file, &model, &canonical) else {
        eprintln!("nx-dsl i18n extract: the program does not lower; fix it first");
        return ExitCode::from(1);
    };
    let keys: Vec<String> = {
        let reader = match nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&lowered.nxir) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("nx-dsl i18n extract: self-read failed: {e:?}");
                return ExitCode::from(1);
            }
        };
        let Ok(root) = reader.root() else {
            eprintln!("nx-dsl i18n extract: self-read failed");
            return ExitCode::from(1);
        };
        let symbols: Vec<String> = root
            .get_symbols()
            .map(|list| {
                list.iter()
                    .map(|s| {
                        s.ok().and_then(|t| t.to_str().ok()).map(String::from).unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default();
        nexus_dsl_runtime::i18n::key_names(root, &symbols)
    };

    // Preserve translations already present in the output file.
    let existing: Vec<(String, String)> = std::fs::read_to_string(out)
        .ok()
        .and_then(|text| parse_flat_json(&text))
        .unwrap_or_default();

    let mut json = String::from("{\n");
    for (i, key) in keys.iter().enumerate() {
        let value =
            existing.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default();
        json.push_str(&format!(
            "  \"{}\": \"{}\"{}\n",
            escape(key),
            escape(&value),
            if i + 1 < keys.len() { "," } else { "" }
        ));
    }
    json.push_str("}\n");
    if let Some(parent) = Path::new(out).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(out, json) {
        eprintln!("nx-dsl: cannot write `{out}`: {e}");
        return ExitCode::from(2);
    }
    println!("{out}: {} key(s)", keys.len());
    ExitCode::SUCCESS
}

fn compile(input: &str, out: &str) -> ExitCode {
    let text = match std::fs::read_to_string(input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nx-dsl: cannot read `{input}`: {e}");
            return ExitCode::from(2);
        }
    };
    let Some(mut entries) = parse_flat_json(&text) else {
        eprintln!("nx-dsl i18n compile: `{input}` is not a flat string catalog");
        return ExitCode::from(1);
    };
    // Untranslated (empty) entries are omitted — the runtime chain falls
    // through to the next catalog / pseudo-locale instead of showing "".
    entries.retain(|(_, v)| !v.is_empty());
    entries.sort();

    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(b"NXC1");
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (key, value) in &entries {
        bytes.extend_from_slice(&(key.len() as u32).to_le_bytes());
        bytes.extend_from_slice(key.as_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    if let Err(e) = std::fs::write(out, &bytes) {
        eprintln!("nx-dsl: cannot write `{out}`: {e}");
        return ExitCode::from(2);
    }
    println!("{out}: {} entrie(s), {} bytes", entries.len(), bytes.len());
    ExitCode::SUCCESS
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// Minimal flat `{"key": "value", …}` parser (catalogs are flat by contract;
/// anything else = `None`).
pub fn parse_flat_json(text: &str) -> Option<Vec<(String, String)>> {
    let mut chars = text.chars().peekable();
    let skip_ws = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
    };
    let parse_string = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| -> Option<String> {
        if chars.next()? != '"' {
            return None;
        }
        let mut out = String::new();
        loop {
            match chars.next()? {
                '\\' => match chars.next()? {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    _ => return None,
                },
                '"' => return Some(out),
                ch => out.push(ch),
            }
        }
    };
    skip_ws(&mut chars);
    if chars.next()? != '{' {
        return None;
    }
    let mut entries = Vec::new();
    loop {
        skip_ws(&mut chars);
        match chars.peek()? {
            '}' => {
                chars.next();
                return Some(entries);
            }
            '"' => {
                let key = parse_string(&mut chars)?;
                skip_ws(&mut chars);
                if chars.next()? != ':' {
                    return None;
                }
                skip_ws(&mut chars);
                let value = parse_string(&mut chars)?;
                entries.push((key, value));
                skip_ws(&mut chars);
                match chars.peek()? {
                    ',' => {
                        chars.next();
                    }
                    '}' => {}
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
}
