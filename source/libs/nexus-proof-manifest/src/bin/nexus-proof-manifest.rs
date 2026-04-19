// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nexus-proof-manifest` host CLI — TASK-0023B Cut P4-05.
//!
//! Single binary that surfaces `proof-manifest.toml` queries to host-side
//! harnesses (`scripts/qemu-test.sh`, `tools/os2vm.sh`, CI). Replaces the
//! ad-hoc `expected_sequence` / `REQUIRE_*` bash arrays with manifest
//! lookups so the manifest is the executable SSOT.
//!
//! Subcommands (all default to the workspace `proof-manifest.toml`):
//!
//!   list-markers   --profile=<name> [--phase=<name>] [--format=lines|json]
//!   list-env       --profile=<name>                  [--format=shell|json]
//!   list-forbidden --profile=<name>                  [--format=lines|json]
//!   list-phases    --profile=<name>                  [--format=lines|json]
//!   verify         --manifest=<path>
//!   verify-uart    --profile=<name> --uart=<path>    [--format=lines|json]
//!
//! `verify-uart` is the deny-by-default analyzer (P4-09): any UART line
//! containing a manifest-declared marker that the active profile does NOT
//! list as expected → fail. Forbidden markers are also checked (any
//! occurrence → fail).
//!
//! Exit codes: 0 success; 1 schema/usage/verification error; 2 missing
//! manifest or uart log.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P4-05+; harness consumption migrates incrementally)
//! API_STABILITY: Internal (CLI shape stable across cuts P4-05+)
//! TEST_COVERAGE: integration tests under `tests/cli_*.rs`
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_proof_manifest::{parse, Manifest};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(msg)) => {
            eprintln!("usage error: {msg}");
            eprintln!();
            eprintln!("{}", USAGE);
            ExitCode::from(1)
        }
        Err(CliError::Manifest(msg)) => {
            eprintln!("manifest error: {msg}");
            ExitCode::from(1)
        }
        Err(CliError::Io(msg)) => {
            eprintln!("io error: {msg}");
            ExitCode::from(2)
        }
    }
}

const USAGE: &str = r#"nexus-proof-manifest <subcommand> [options]

subcommands:
  list-markers   --profile=<name> [--phase=<name>] [--format=lines|json]
  list-env       --profile=<name>                  [--format=shell|json]
  list-forbidden --profile=<name>                  [--format=lines|json]
  list-phases    --profile=<name>                  [--format=lines|json]
  verify         [--manifest=<path>]
  verify-uart    --profile=<name> --uart=<path>    [--format=lines|json]

options shared by all subcommands:
  --manifest=<path>   path to proof-manifest.toml
                      (default: source/apps/selftest-client/proof-manifest.toml)
  --uart=<path>       path to UART transcript (verify-uart only)
  --format=<fmt>      output format; defaults vary per subcommand

exit codes: 0 ok | 1 schema/usage/verification | 2 missing file"#;

enum CliError {
    Usage(String),
    Manifest(String),
    Io(String),
}

fn run(argv: &[String]) -> Result<(), CliError> {
    if argv.len() < 2 {
        return Err(CliError::Usage("missing subcommand".into()));
    }
    let sub = argv[1].as_str();
    let opts = parse_opts(&argv[2..])?;
    let manifest = load_manifest(opts.manifest.as_deref())?;
    match sub {
        "list-markers" => cmd_list_markers(&manifest, &opts),
        "list-env" => cmd_list_env(&manifest, &opts),
        "list-forbidden" => cmd_list_forbidden(&manifest, &opts),
        "list-phases" => cmd_list_phases(&manifest, &opts),
        "verify" => Ok(()),
        "verify-uart" => cmd_verify_uart(&manifest, &opts),
        other => Err(CliError::Usage(format!("unknown subcommand `{other}`"))),
    }
}

#[derive(Debug, Default)]
struct Opts {
    manifest: Option<PathBuf>,
    profile: Option<String>,
    phase: Option<String>,
    format: Option<String>,
    uart: Option<PathBuf>,
}

fn parse_opts(args: &[String]) -> Result<Opts, CliError> {
    let mut o = Opts::default();
    for arg in args {
        let (k, v) = arg
            .split_once('=')
            .ok_or_else(|| CliError::Usage(format!("expected --key=value, got `{arg}`")))?;
        match k {
            "--manifest" => o.manifest = Some(PathBuf::from(v)),
            "--profile" => o.profile = Some(v.to_string()),
            "--phase" => o.phase = Some(v.to_string()),
            "--format" => o.format = Some(v.to_string()),
            "--uart" => o.uart = Some(PathBuf::from(v)),
            other => return Err(CliError::Usage(format!("unknown option `{other}`"))),
        }
    }
    Ok(o)
}

fn load_manifest(path: Option<&std::path::Path>) -> Result<Manifest, CliError> {
    let p: PathBuf = path
        .map(PathBuf::from)
        .unwrap_or_else(default_manifest_path);
    let src = std::fs::read_to_string(&p)
        .map_err(|e| CliError::Io(format!("read {}: {e}", p.display())))?;
    parse(&src).map_err(|e| CliError::Manifest(e.to_string()))
}

fn default_manifest_path() -> PathBuf {
    PathBuf::from("source/apps/selftest-client/proof-manifest.toml")
}

fn require_profile(opts: &Opts) -> Result<&str, CliError> {
    opts.profile
        .as_deref()
        .ok_or_else(|| CliError::Usage("--profile=<name> required".into()))
}

fn cmd_list_markers(m: &Manifest, opts: &Opts) -> Result<(), CliError> {
    let profile = require_profile(opts)?;
    if !m.profiles.contains_key(profile) {
        return Err(CliError::Manifest(format!(
            "profile `{profile}` not declared"
        )));
    }
    let format = opts.format.as_deref().unwrap_or("lines");
    let phase_filter = opts.phase.as_deref();

    let active: Vec<_> = m
        .expected_markers(profile)
        .filter(|m| phase_filter.is_none_or(|p| m.phase == p))
        .collect();

    match format {
        "lines" => {
            for marker in active {
                println!("{}", marker.literal);
            }
        }
        "json" => {
            print!("[");
            for (i, marker) in active.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!(
                    "{{\"literal\":{},\"phase\":{}}}",
                    json_string(&marker.literal),
                    json_string(&marker.phase)
                );
            }
            println!("]");
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown --format `{other}` for list-markers (lines|json)"
            )))
        }
    }
    Ok(())
}

fn cmd_list_env(m: &Manifest, opts: &Opts) -> Result<(), CliError> {
    let profile = require_profile(opts)?;
    let env = m
        .resolve_env_chain(profile)
        .map_err(|e| CliError::Manifest(e.to_string()))?;
    let format = opts.format.as_deref().unwrap_or("shell");
    match format {
        "shell" => {
            for (k, v) in env {
                println!("{k}={}", shell_quote(&v));
            }
        }
        "json" => {
            print!("{{");
            for (i, (k, v)) in env.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!("{}:{}", json_string(k), json_string(v));
            }
            println!("}}");
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown --format `{other}` for list-env (shell|json)"
            )))
        }
    }
    Ok(())
}

fn cmd_list_forbidden(m: &Manifest, opts: &Opts) -> Result<(), CliError> {
    let profile = require_profile(opts)?;
    if !m.profiles.contains_key(profile) {
        return Err(CliError::Manifest(format!(
            "profile `{profile}` not declared"
        )));
    }
    let format = opts.format.as_deref().unwrap_or("lines");
    let forbidden: Vec<_> = m.forbidden_markers(profile).collect();
    match format {
        "lines" => {
            for marker in forbidden {
                println!("{}", marker.literal);
            }
        }
        "json" => {
            print!("[");
            for (i, marker) in forbidden.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!("{}", json_string(&marker.literal));
            }
            println!("]");
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown --format `{other}` for list-forbidden (lines|json)"
            )))
        }
    }
    Ok(())
}

fn cmd_list_phases(m: &Manifest, opts: &Opts) -> Result<(), CliError> {
    let _profile = require_profile(opts)?;
    let format = opts.format.as_deref().unwrap_or("lines");
    let mut entries: Vec<(&String, &nexus_proof_manifest::Phase)> = m.phases.iter().collect();
    entries.sort_by_key(|(_, p)| p.order);
    match format {
        "lines" => {
            for (name, _) in entries {
                println!("{name}");
            }
        }
        "json" => {
            print!("[");
            for (i, (name, p)) in entries.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!("{{\"name\":{},\"order\":{}}}", json_string(name), p.order);
            }
            println!("]");
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown --format `{other}` for list-phases (lines|json)"
            )))
        }
    }
    Ok(())
}

/// P4-09: deny-by-default analyzer. Reads a UART transcript and reports
/// any manifest-declared marker that appears in the log but is NOT in the
/// active profile's expected-marker projection (= "unexpected"), and any
/// marker that the profile lists as `forbidden_when` (= "forbidden").
///
/// Returns a structured failure (exit 1) if either set is non-empty;
/// returns success (exit 0) when the UART transcript is consistent with
/// the profile's marker contract.
///
/// `--format=lines` (default) emits human-readable lines; `--format=json`
/// emits a structured `{ "unexpected": [...], "forbidden": [...] }` object
/// for downstream tooling.
fn cmd_verify_uart(m: &Manifest, opts: &Opts) -> Result<(), CliError> {
    let profile = require_profile(opts)?;
    if !m.profiles.contains_key(profile) {
        return Err(CliError::Manifest(format!(
            "profile `{profile}` not declared"
        )));
    }
    let uart_path = opts
        .uart
        .as_deref()
        .ok_or_else(|| CliError::Usage("--uart=<path> required for verify-uart".into()))?;
    let uart_src = std::fs::read_to_string(uart_path)
        .map_err(|e| CliError::Io(format!("read {}: {e}", uart_path.display())))?;

    // Build the per-profile sets up-front so the scan is O(lines * markers).
    let expected: std::collections::BTreeSet<String> = m
        .expected_markers(profile)
        .map(|mk| mk.literal.clone())
        .collect();
    let forbidden: std::collections::BTreeSet<String> = m
        .forbidden_markers(profile)
        .map(|mk| mk.literal.clone())
        .collect();
    let universe: Vec<&str> = m.markers.iter().map(|mk| mk.literal.as_str()).collect();

    // Walk the UART log; for each line, find every manifest literal it
    // contains (substring match — UART output may have a `[QEMU] ` prefix
    // or inline noise). Collect violations as ordered, deduplicated sets
    // so the output is deterministic across runs.
    let mut unexpected: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut forbidden_hits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for line in uart_src.lines() {
        for lit in &universe {
            if line.contains(lit) {
                if forbidden.contains(*lit) {
                    forbidden_hits.insert((*lit).to_string());
                } else if !expected.contains(*lit) {
                    unexpected.insert((*lit).to_string());
                }
            }
        }
    }

    let format = opts.format.as_deref().unwrap_or("lines");
    let has_violations = !unexpected.is_empty() || !forbidden_hits.is_empty();
    match format {
        "lines" => {
            if !forbidden_hits.is_empty() {
                eprintln!("[verify-uart] forbidden markers present (profile={profile}):");
                for lit in &forbidden_hits {
                    eprintln!("  - {lit}");
                }
            }
            if !unexpected.is_empty() {
                eprintln!("[verify-uart] unexpected markers (profile={profile}):");
                for lit in &unexpected {
                    eprintln!("  - {lit}");
                }
            }
            if !has_violations {
                println!("[verify-uart] ok: profile={profile}, uart={}", uart_path.display());
            }
        }
        "json" => {
            print!("{{\"profile\":{},\"unexpected\":[", json_string(profile));
            for (i, lit) in unexpected.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!("{}", json_string(lit));
            }
            print!("],\"forbidden\":[");
            for (i, lit) in forbidden_hits.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!("{}", json_string(lit));
            }
            println!("]}}");
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown --format `{other}` for verify-uart (lines|json)"
            )))
        }
    }

    if has_violations {
        return Err(CliError::Manifest(format!(
            "verify-uart failed (profile={profile}): {} unexpected, {} forbidden",
            unexpected.len(),
            forbidden_hits.len()
        )));
    }
    Ok(())
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '='))
    {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}
