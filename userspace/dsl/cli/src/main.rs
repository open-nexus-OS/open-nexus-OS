// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nx-dsl` — the DSL toolchain backend behind the `nx dsl` shim
//! (`tools/nx/src/commands/dsl.rs`, delegation via `NX_DSL_BACKEND`).
//! Verbs: `fmt`, `lint`, `build` (shim contract) + `check`, `hash`, `explain`.
//! OWNERS: @ui @runtime
//! STATUS: Functional (TASK-0075 v0.1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host suite `tests/dsl_v0_1a_host`
//!
//! Exit codes: 0 ok, 1 diagnostics/violations, 2 usage/IO errors.

use nexus_dsl_core::{check_file, format_file, has_errors, lower_file, parse_file, Severity};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod i18n_cmd;
mod scaffold;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verb = args.first().map(String::as_str).unwrap_or("");
    let rest = &args[1.min(args.len())..];
    match verb {
        "fmt" => cmd_fmt(rest),
        "lint" | "check" => cmd_lint(rest, verb == "check"),
        "build" => cmd_build(rest),
        "run" => cmd_run(rest),
        "hash" => cmd_hash(rest),
        "explain" => cmd_explain(rest),
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

fn report(path: &str, source: &str, diag: &nexus_dsl_core::Diagnostic) {
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
    // Project directory: collect ui/**.nx (sorted paths, deterministic).
    let mut files: Vec<nexus_dsl_core::SourceFile> = Vec::new();
    let root = Path::new(path);
    let mut stack = vec![root.join("ui")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("nx") {
                let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                let source = std::fs::read_to_string(&p).map_err(|e| {
                    eprintln!("nx-dsl: cannot read `{}`: {e}", p.display());
                    ExitCode::from(2)
                })?;
                files.push(nexus_dsl_core::SourceFile { path: rel, source });
            }
        }
    }
    if files.is_empty() {
        eprintln!("nx-dsl: no ui/**.nx files under `{path}`");
        return Err(ExitCode::from(2));
    }
    let canonical = nexus_dsl_core::canonical_source_set(&files);
    let merged = nexus_dsl_core::merge_project(&files).map_err(|diag| {
        eprintln!("{path}: error[{}]: {}", diag.code, diag.message);
        ExitCode::from(1)
    })?;
    Ok((merged, canonical, String::new()))
}

fn nx_files(args: &[String]) -> Vec<String> {
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
fn cmd_run(args: &[String]) -> ExitCode {
    let flag = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let route = flag("--route");
    let locale_name = flag("--locale");
    let profile = flag("--profile").unwrap_or_else(|| String::from("desktop"));
    let transcript_path = flag("--transcript");
    let dispatch_case = flag("--dispatch");
    let flag_values: Vec<String> =
        ["--route", "--locale", "--profile", "--transcript", "--dispatch"]
            .iter()
            .filter_map(|n| flag(n))
            .collect();
    let files: Vec<String> =
        nx_files(args).into_iter().filter(|f| !flag_values.contains(f)).collect();
    let Some(path) = files.first() else {
        eprintln!("nx-dsl run: no input file");
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

    // Environment per profile.
    let device = match profile.as_str() {
        "desktop" => nexus_dsl_runtime::FixtureEnv::desktop(),
        "phone" => nexus_dsl_runtime::FixtureEnv::phone("portrait"),
        "tablet" => nexus_dsl_runtime::FixtureEnv::tablet("landscape"),
        "tv" => nexus_dsl_runtime::FixtureEnv {
            profile: "tv",
            posture: "",
            orientation: "landscape",
            shell_mode: "tv",
            size_class: "wide",
            dpi_class: "normal",
            input: &["remote"],
        },
        other => {
            eprintln!("nx-dsl run: unknown profile `{other}`");
            return ExitCode::from(2);
        }
    };

    // Program key names for locale sources.
    let reader = match nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&lowered.nxir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nx-dsl run: self-read failed: {e:?}");
            return ExitCode::from(1);
        }
    };
    let Ok(root) = reader.root() else {
        eprintln!("nx-dsl run: self-read failed");
        return ExitCode::from(1);
    };
    let symbols: Vec<String> = root
        .get_symbols()
        .map(|list| {
            list.iter()
                .map(|s| s.ok().and_then(|t| t.to_str().ok()).map(String::from).unwrap_or_default())
                .collect()
        })
        .unwrap_or_default();
    let key_names = nexus_dsl_runtime::i18n::key_names(root, &symbols);
    let key_refs: Vec<&str> = key_names.iter().map(String::as_str).collect();

    // Locale: <appdir>/i18n/<locale>.json (or .nxc) chained onto the pseudo-locale.
    let mut catalogs: Vec<nexus_dsl_runtime::Catalog> = Vec::new();
    if let Some(locale) = &locale_name {
        let base = Path::new(path).join("i18n");
        let json = base.join(format!("{locale}.json"));
        let nxc = base.join(format!("{locale}.nxc"));
        let catalog = if nxc.exists() {
            std::fs::read(&nxc)
                .ok()
                .and_then(|b| nexus_dsl_runtime::Catalog::from_binary(&key_refs, &b))
        } else {
            std::fs::read_to_string(&json).ok().and_then(|text| {
                let entries = i18n_cmd::parse_flat_json(&text)?;
                let refs: Vec<(&str, &str)> =
                    entries.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                Some(nexus_dsl_runtime::Catalog::from_entries(&key_refs, &refs))
            })
        };
        match catalog {
            Some(c) => catalogs.push(c),
            None => {
                eprintln!(
                    "nx-dsl run: no readable catalog for locale `{locale}` under `{}`",
                    base.display()
                );
                return ExitCode::from(1);
            }
        }
    }
    let catalog_refs: Vec<&nexus_dsl_runtime::Catalog> = catalogs.iter().collect();
    let locale = nexus_dsl_runtime::LocaleChain::new(&catalog_refs, &key_names);

    let tokens = nexus_theme_tokens::BaseTokens;
    let mut view = match nexus_dsl_runtime::View::mount(&lowered.nxir, &tokens, &device, &locale) {
        Ok(view) => view,
        Err(e) => {
            eprintln!("nx-dsl run: view mount failed: {e:?}");
            return ExitCode::from(1);
        }
    };

    if let Some(route) = &route {
        if let Err(e) = view.navigate(&tokens, &device, &locale, route) {
            eprintln!("nx-dsl run: navigate `{route}` failed: {e:?}");
            return ExitCode::from(1);
        }
    }

    // Optional single dispatch, replayed against a transcript host.
    let mut transcript_host = match &transcript_path {
        Some(t) => match std::fs::read_to_string(t) {
            Ok(text) => match nexus_dsl_runtime::svc::TranscriptHost::parse(&text) {
                Ok(host) => Some(host),
                Err(line) => {
                    eprintln!("nx-dsl run: `{t}` line {line}: malformed transcript entry");
                    return ExitCode::from(1);
                }
            },
            Err(e) => {
                eprintln!("nx-dsl: cannot read `{t}`: {e}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    if let Some(case) = &dispatch_case {
        let Some((event, case_idx)) = find_case(&view, case) else {
            eprintln!("nx-dsl run: `{case}` is not a declared event case");
            return ExitCode::from(1);
        };
        let result = match transcript_host.as_mut() {
            Some(host) => {
                view.dispatch(&tokens, &device, &locale, host, event, case_idx, Vec::new())
            }
            None => {
                let mut noio = nexus_dsl_runtime::NoIo;
                view.dispatch(&tokens, &device, &locale, &mut noio, event, case_idx, Vec::new())
            }
        };
        if let Err(e) = result {
            eprintln!("nx-dsl run: dispatch `{case}` failed: {e:?}");
            return ExitCode::from(1);
        }
    }
    if let Some(host) = &transcript_host {
        if !host.is_clean() {
            eprintln!("nx-dsl run: transcript replay had misses: {:?}", host.misses);
            return ExitCode::from(1);
        }
    }

    println!(
        "mounted: hash {}, profile {profile}, {} deps",
        hex(&lowered.program_hash),
        view.deps().len()
    );
    for text in scene_texts(view.scene()) {
        println!("text: {text}");
    }
    ExitCode::SUCCESS
}

/// Resolves a bare case name against the mounted program's events.
fn find_case(view: &nexus_dsl_runtime::View<'_>, case: &str) -> Option<(u32, u32)> {
    let runtime = view.runtime();
    let root = runtime.reader().root().ok()?;
    let case_sym = runtime.symbols().iter().position(|s| s == case)? as u32;
    for (e, decl) in root.get_events().ok()?.iter().enumerate() {
        for (c, case_decl) in decl.get_cases().ok()?.iter().enumerate() {
            if case_decl.get_name() == case_sym {
                return Some((e as u32, c as u32));
            }
        }
    }
    None
}

/// Text nodes of the emitted scene, pre-order (the deterministic summary).
fn scene_texts(scene: &nexus_layout_types::LayoutNode) -> Vec<String> {
    fn walk(node: &nexus_layout_types::LayoutNode, out: &mut Vec<String>) {
        use nexus_layout_types::LayoutNode as N;
        match node {
            N::Text(text, _) => out.push(String::from(text.content.as_str())),
            N::Stack(_, _, children) | N::Grid(_, _, children) => {
                for child in children {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(scene, &mut out);
    out
}

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
            .and_then(|r| r.root().map(|root| root.get_program_hash().map(|h| hex(h)).ok()))
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

fn cmd_explain(args: &[String]) -> ExitCode {
    let Some(code) = args.first() else {
        eprintln!("nx-dsl explain: pass a diagnostic code (e.g. NX0405)");
        return ExitCode::from(2);
    };
    let text = match code.as_str() {
        "NX0001" => "Unexpected character in the source.",
        "NX0002" => "String literal not closed before end of line/file.",
        "NX0003" => "Source file exceeds the size bound.",
        "NX0004" => "Identifier exceeds the length bound.",
        "NX0005" => "Numeric literal out of range (Int is i64; Fx is Q32.32).",
        "NX0101" => "The parser found a token it cannot use here; the message names what it expected.",
        "NX0103" => "The same property is set twice on one node.",
        "NX0104" => "Content after the last declaration.",
        "NX0105" => "Structural nesting exceeds the bound (64 levels).",
        "NX0106" => "`reduce`/`match` needs at least one arm.",
        "NX0107" => "Route paths start with `/`.",
        "NX0201" => "Name is not defined anywhere visible.",
        "NX0202" => "The same name is defined twice.",
        "NX0203" => "Two imports define the same symbol.",
        "NX0204" => "Not a known widget or a declared component.",
        "NX0205" => "Not a catalog modifier (see docs/dev/dsl/modifiers.md).",
        "NX0206" => "Not a declared event type/case.",
        "NX0207" => "Not a platform service (the surface is generated from dsl_services.capnp).",
        "NX0208" => "The service exists but has no such method.",
        "NX0301" => "Types don't match.",
        "NX0302" => "Wrong number of arguments/bindings.",
        "NX0303" => "Unknown field/prop on this type.",
        "NX0304" => "`reduce`/`match` must cover every case.",
        "NX0305" => "Not a case of this enum/event.",
        "NX0306" => "Unknown type name.",
        "NX0307" => "A constant expression is required here.",
        "NX0401" => "Collection items need a stable `.key(expr)` on the template root.",
        "NX0402" => "Interactive nodes need an accessible name (label prop or `.label(…)`).",
        "NX0403" => "A modifier is applied twice on one node.",
        "NX0404" => "`for` needs a statically bounded iterable; use `List(expr) { item in … }` for data.",
        "NX0405" => "Reducers are pure: no IO, no `svc.*`, no dispatch — use an `@effect`.",
        "NX0406" => "Profile branch without a final `else`: add the default branch. (Warning)",
        "NX0407" => "A service result is ignored; bind and handle it. (Warning in v0.1)",
        "NX0408" => "The same route path is declared twice.",
        "NX0409" => "Service calls should pass `timeoutMs:` explicitly. (Warning in v0.1)",
        "NX0410" => "Query outside the v1 shape: eq/>=/<= only, ranges on the orderBy column, literal-or-param values, limit 1..=1000.",
        "NX0501" => "Valid syntax, but outside the v0.1 lowering subset (see the task notes).",
        _ => {
            eprintln!("nx-dsl explain: unknown code `{code}`");
            return ExitCode::from(1);
        }
    };
    println!("{code}: {text}");
    ExitCode::SUCCESS
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
