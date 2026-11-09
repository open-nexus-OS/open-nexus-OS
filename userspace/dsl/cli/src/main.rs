// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nx-dsl` — the DSL toolchain backend behind the `nx dsl` shim
//! (`tools/nx/src/commands/dsl.rs`, delegation via `NX_DSL_BACKEND`).
//! Verbs: `fmt`, `lint`, `build` (shim contract) + `check`, `run`, `render`, `hash`, `explain`.
//! OWNERS: @ui @runtime
//! STATUS: Functional (TASK-0075 v0.1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host suite `tests/dsl_v0_1a_host`
//!
//! Exit codes: 0 ok, 1 diagnostics/violations, 2 usage/IO errors.

use nexus_dsl_core::{check_file, format_file, has_errors, lower_file, parse_file, Severity};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod explain;
mod i18n_cmd;
mod png;
mod prepare;
mod render;
mod run;
mod scaffold;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verb = args.first().map(String::as_str).unwrap_or("");
    let rest = &args[1.min(args.len())..];
    match verb {
        "fmt" => cmd_fmt(rest),
        "lint" | "check" => cmd_lint(rest, verb == "check"),
        "build" => cmd_build(rest),
        "run" => run::cmd_run(rest),
        "render" => render::cmd_render(rest),
        "hash" => cmd_hash(rest),
        "explain" => explain::cmd_explain(rest),
        "i18n" => i18n_cmd::cmd_i18n(rest),
        "init" => scaffold::cmd_init(rest),
        "add" => scaffold::cmd_add(rest),
        _ => {
            eprintln!(
                "usage: nx-dsl <verb> [flags] <files…>\n\
                 verbs:\n\
                 \x20 fmt [--check] <files…>     format in place (or verify)\n\
                 \x20 lint [--deny-warn] <files…> parse + check, report diagnostics\n\
                 \x20 check <files…>              lint + lowering dry-run\n\
                 \x20 build [-o DIR] <file>       emit canonical .nxir (+ --emit-json summary)\n\
                 \x20 run <file>                  compile + mount + first-frame summary\n\
                 \x20 render -o OUT.png [--theme light|dark] [--solid] [--size WxH] <app|file>  host preview\n\
                 \x20 hash <file.nx|file.nxir>    print the program hash\n\
                 \x20 explain <NXcode>            explain a diagnostic code"
            );
            ExitCode::from(2)
        }
    }
}

fn read(path: &str) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|e| {
        eprintln!("nx-dsl: cannot read `{path}`: {e}");
        ExitCode::from(2)
    })
}

pub(crate) fn report(path: &str, source: &str, diag: &nexus_dsl_core::Diagnostic) {
    let (line, col) = nexus_dsl_core::diag::line_col(source, diag.span.start);
    let sev = match diag.severity() {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    eprintln!("{path}:{line}:{col}: {sev}[{}]: {}", diag.code, diag.message);
}

/// Loads a `.nx` file or an app directory (walks `ui/**.nx` in sorted order
/// through `merge_project` — platform overrides included).
pub(crate) fn load_input(
    path: &str,
) -> Result<(nexus_dsl_core::ast::File, String, String), ExitCode> {
    let meta = std::fs::metadata(path).map_err(|e| {
        eprintln!("nx-dsl: cannot read `{path}`: {e}");
        ExitCode::from(2)
    })?;
    if meta.is_file() {
        let source = read(path)?;
        let file = match parse_file(&source) {
            Ok(f) => f,
            Err(diag) => {
                report(path, &source, &diag);
                return Err(ExitCode::from(1));
            }
        };
        let canonical = format_file(&file);
        return Ok((file, canonical, source));
    }
    // Project directory: the ONE project loader (ui/**.nx + the referenced
    // library components, merged with platform overrides).
    let (merged, canonical) = nexus_dsl_core::load_project(Path::new(path)).map_err(|e| {
        eprintln!("nx-dsl: {path}: {e}");
        ExitCode::from(1)
    })?;
    Ok((merged, canonical, String::new()))
}

pub(crate) fn nx_files(args: &[String]) -> Vec<String> {
    args.iter().filter(|a| !a.starts_with("--") && !a.starts_with('-')).cloned().collect()
}

fn cmd_fmt(args: &[String]) -> ExitCode {
    let check_only = args.iter().any(|a| a == "--check");
    let files = nx_files(args);
    if files.is_empty() {
        eprintln!("nx-dsl fmt: no input files");
        return ExitCode::from(2);
    }
    let mut dirty = false;
    for path in &files {
        let source = match read(path) {
            Ok(s) => s,
            Err(code) => return code,
        };
        let file = match parse_file(&source) {
            Ok(f) => f,
            Err(diag) => {
                report(path, &source, &diag);
                return ExitCode::from(1);
            }
        };
        let formatted = format_file(&file);
        if formatted != source {
            dirty = true;
            if check_only {
                eprintln!("{path}: needs formatting");
            } else if let Err(e) = std::fs::write(path, &formatted) {
                eprintln!("nx-dsl: cannot write `{path}`: {e}");
                return ExitCode::from(2);
            }
        }
    }
    if check_only && dirty {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_lint(args: &[String], and_lower: bool) -> ExitCode {
    let deny_warn = args.iter().any(|a| a == "--deny-warn");
    let files = nx_files(args);
    if files.is_empty() {
        eprintln!("nx-dsl: no input files");
        return ExitCode::from(2);
    }
    let mut failed = false;
    for path in &files {
        let source = match read(path) {
            Ok(s) => s,
            Err(code) => return code,
        };
        match parse_file(&source) {
            Err(diag) => {
                report(path, &source, &diag);
                failed = true;
            }
            Ok(file) => {
                let (model, diags) = check_file(&file);
                for diag in &diags {
                    report(path, &source, diag);
                }
                if has_errors(&diags) || (deny_warn && !diags.is_empty()) {
                    failed = true;
                } else if and_lower {
                    let canonical = format_file(&file);
                    if let Err(diag) = lower_file(&file, &model, &canonical) {
                        report(path, &source, &diag);
                        failed = true;
                    }
                }
            }
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_build(args: &[String]) -> ExitCode {
    let emit_json = args.iter().any(|a| a == "--emit-json");
    let out_dir: PathBuf = args
        .iter()
        .position(|a| a == "-o")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/dsl"));
    let files: Vec<String> =
        nx_files(args).into_iter().filter(|f| f != out_dir.to_str().unwrap_or("")).collect();
    let Some(path) = files.first() else {
        eprintln!("nx-dsl build: no input file");
        return ExitCode::from(2);
    };
    let (file, canonical, source) = match load_input(path) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let (model, diags) = check_file(&file);
    for diag in &diags {
        report(path, &source, diag);
    }
    if has_errors(&diags) {
        return ExitCode::from(1);
    }
    let lowered = match lower_file(&file, &model, &canonical) {
        Ok(l) => l,
        Err(diag) => {
            report(path, &source, &diag);
            return ExitCode::from(1);
        }
    };
    let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("program");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("nx-dsl: cannot create `{}`: {e}", out_dir.display());
        return ExitCode::from(2);
    }
    let nxir_path = out_dir.join(format!("{stem}.nxir"));
    if let Err(e) = std::fs::write(&nxir_path, &lowered.nxir) {
        eprintln!("nx-dsl: cannot write `{}`: {e}", nxir_path.display());
        return ExitCode::from(2);
    }
    if emit_json {
        let json_path = out_dir.join(format!("{stem}.nxir.json"));
        let summary = summary_json(&lowered);
        if let Err(e) = std::fs::write(&json_path, summary) {
            eprintln!("nx-dsl: cannot write `{}`: {e}", json_path.display());
            return ExitCode::from(2);
        }
    }
    println!(
        "{}: {} bytes, hash {}",
        nxir_path.display(),
        lowered.nxir.len(),
        hex(&lowered.program_hash)
    );
    ExitCode::SUCCESS
}

/// Deterministic derived view (goldens/debug; never consumed at runtime).
fn summary_json(lowered: &nexus_dsl_core::Lowered) -> String {
    use nexus_dsl_ir::read::ProgramReader;
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"programHash\": \"{}\",\n", hex(&lowered.program_hash)));
    out.push_str(&format!("  \"bytes\": {},\n", lowered.nxir.len()));
    if let Ok(reader) = ProgramReader::from_canonical_bytes(&lowered.nxir) {
        if let Ok(root) = reader.root() {
            out.push_str(&format!(
                "  \"schema\": \"{}.{}\",\n",
                root.get_schema_version_major(),
                root.get_schema_version_minor()
            ));
            out.push_str(&format!("  \"entryPage\": {},\n", root.get_entry_page()));
            for (field, len) in [
                ("stores", root.get_stores().map(|l| l.len()).unwrap_or(0)),
                ("events", root.get_events().map(|l| l.len()).unwrap_or(0)),
                ("reducers", root.get_reducers().map(|l| l.len()).unwrap_or(0)),
                ("effects", root.get_effects().map(|l| l.len()).unwrap_or(0)),
                ("components", root.get_components().map(|l| l.len()).unwrap_or(0)),
                ("routes", root.get_routes().map(|l| l.len()).unwrap_or(0)),
                ("i18nKeys", root.get_i18n_keys().map(|l| l.len()).unwrap_or(0)),
            ] {
                out.push_str(&format!("  \"{field}\": {len},\n"));
            }
            if let Ok(symbols) = root.get_symbols() {
                out.push_str("  \"symbols\": [");
                for (i, symbol) in symbols.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format!(
                        "\"{}\"",
                        symbol.ok().and_then(|s| s.to_str().ok()).unwrap_or("")
                    ));
                }
                out.push_str("]\n");
            }
        }
    }
    out.push_str("}\n");
    out
}

/// Headless run: compile → validate → mount → (optional) navigate, locale,
/// profile, transcript-backed dispatch — then a deterministic text summary.
/// The graphical snapshot harness lives in `tests/dsl_goldens`.
fn cmd_hash(args: &[String]) -> ExitCode {
    let files = nx_files(args);
    let Some(path) = files.first() else {
        eprintln!("nx-dsl hash: no input file");
        return ExitCode::from(2);
    };
    if path.ends_with(".nxir") {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("nx-dsl: cannot read `{path}`: {e}");
                return ExitCode::from(2);
            }
        };
        match nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&bytes)
            .and_then(|r| r.root().map(|root| root.get_program_hash().map(hex).ok()))
        {
            Ok(Some(h)) => {
                println!("{h}");
                ExitCode::SUCCESS
            }
            _ => {
                eprintln!("nx-dsl: `{path}` is not a readable .nxir");
                ExitCode::from(1)
            }
        }
    } else {
        let source = match read(path) {
            Ok(s) => s,
            Err(code) => return code,
        };
        let file = match parse_file(&source) {
            Ok(f) => f,
            Err(diag) => {
                report(path, &source, &diag);
                return ExitCode::from(1);
            }
        };
        let (model, diags) = check_file(&file);
        if has_errors(&diags) {
            for diag in &diags {
                report(path, &source, diag);
            }
            return ExitCode::from(1);
        }
        let canonical = format_file(&file);
        match lower_file(&file, &model, &canonical) {
            Ok(lowered) => {
                println!("{}", hex(&lowered.program_hash));
                ExitCode::SUCCESS
            }
            Err(diag) => {
                report(path, &source, &diag);
                ExitCode::from(1)
            }
        }
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
