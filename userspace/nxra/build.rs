// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Bakes `policies/nxra-trust.toml` into `OUT_DIR/trust_baked.rs`
// (RFC-0088 trust anchor: build-time, image-integrity-strong). The file
// format is deliberately narrow — `[[key]]` blocks with `pubkey` (hex64)
// and `actions` (known labels) — and every violation FAILS THE BUILD:
// a half-parsed trust list must never produce a permissive anchor.

use std::fmt::Write as _;
use std::path::Path;

const ACTIONS: [(&str, &str); 4] = [
    ("slot-switch", "Action::SlotSwitch"),
    ("target-set", "Action::TargetSet"),
    ("fsck-repair", "Action::FsckRepair"),
    ("reset", "Action::Reset"),
];

fn main() {
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        panic!("CARGO_MANIFEST_DIR unset")
    };
    let trust_path = Path::new(&manifest_dir).join("../../policies/nxra-trust.toml");
    println!("cargo::rerun-if-changed={}", trust_path.display());
    let text = std::fs::read_to_string(&trust_path)
        .unwrap_or_else(|err| panic!("nxra-trust.toml unreadable: {err}"));

    let mut entries: Vec<(String, Vec<&'static str>)> = Vec::new();
    let mut pubkey: Option<String> = None;
    let mut actions: Option<Vec<&'static str>> = None;
    let flush = |pubkey: &mut Option<String>,
                 actions: &mut Option<Vec<&'static str>>,
                 entries: &mut Vec<(String, Vec<&'static str>)>| {
        match (pubkey.take(), actions.take()) {
            (None, None) => {}
            (Some(key), Some(acts)) => entries.push((key, acts)),
            _ => panic!("nxra-trust.toml: [[key]] block missing pubkey or actions"),
        }
    };
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[key]]" {
            flush(&mut pubkey, &mut actions, &mut entries);
            continue;
        }
        if let Some(value) = line.strip_prefix("pubkey") {
            let hex = value.trim_start_matches(['=', ' ']).trim().trim_matches('"');
            if hex.len() != 64
                || !hex.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                panic!("nxra-trust.toml: pubkey must be 64 lowercase hex chars, got `{hex}`");
            }
            if pubkey.replace(hex.to_string()).is_some() {
                panic!("nxra-trust.toml: duplicate pubkey in one [[key]] block");
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("actions") {
            let list = value.trim_start_matches(['=', ' ']).trim();
            let inner = list
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
                .unwrap_or_else(|| panic!("nxra-trust.toml: actions must be a [..] list"));
            let mut parsed = Vec::new();
            for item in inner.split(',') {
                let label = item.trim().trim_matches('"');
                if label.is_empty() {
                    continue;
                }
                let variant = ACTIONS
                    .iter()
                    .find(|(name, _)| *name == label)
                    .unwrap_or_else(|| panic!("nxra-trust.toml: unknown action `{label}`"))
                    .1;
                parsed.push(variant);
            }
            if parsed.is_empty() {
                panic!("nxra-trust.toml: a key with no actions is dead weight — remove it");
            }
            if actions.replace(parsed).is_some() {
                panic!("nxra-trust.toml: duplicate actions in one [[key]] block");
            }
            continue;
        }
        panic!("nxra-trust.toml: unrecognized line `{line}` (format is narrow on purpose)");
    }
    flush(&mut pubkey, &mut actions, &mut entries);

    let mut out = String::new();
    out.push_str("/// Build-time trust anchor from policies/nxra-trust.toml (RFC-0088).\n");
    out.push_str("pub static BAKED_TRUST: &[TrustEntry] = &[\n");
    for (hex, acts) in &entries {
        let mut bytes = String::new();
        for i in 0..32 {
            let Ok(byte) = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16) else {
                panic!("nxra-trust.toml: hex re-parse failed (validated above)")
            };
            let _ = write!(bytes, "{byte:#04x}, ");
        }
        let _ = writeln!(
            out,
            "    TrustEntry {{ pubkey: [{bytes}], actions: &[{}] }},",
            acts.join(", ")
        );
    }
    out.push_str("];\n");

    let Ok(out_dir) = std::env::var("OUT_DIR") else { panic!("OUT_DIR unset") };
    if let Err(err) = std::fs::write(Path::new(&out_dir).join("trust_baked.rs"), out) {
        panic!("write trust_baked.rs: {err}");
    }
}
