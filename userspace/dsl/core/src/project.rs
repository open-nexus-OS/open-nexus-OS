// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project merge: multi-file programs + `ui/platform/<profile>/` overrides.
//!
//! Deterministic by construction: files merge in **sorted path order** (never
//! filesystem iteration order), and a platform override never replaces the
//! base page in the IR — it wraps it:
//!
//! ```text
//! ui/platform/phone/pages/Home.nx   ⇒   Page Home {
//! ui/pages/Home.nx                          if device.profile == phone { <override view> }
//!                                            else { <base view> }
//!                                        }
//! ```
//!
//! So ONE canonical `.nxir` serves every profile (the runtime branches on the
//! read-only device environment), and the provenance is carried by
//! `sourceDigest` over the sorted, path-prefixed source set.

use crate::ast::{Decl, Expr, File, Ident, ViewNode};
use crate::diag::{DiagCode, Diagnostic, Span};
use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};

/// One source file of a project (path is repo/app-relative, `/`-separated).
pub struct SourceFile {
    pub path: String,
    pub source: String,
}

/// The profile id if `path` is a platform override (`ui/platform/<p>/…`).
fn override_profile(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("ui/platform/")?;
    let (profile, _) = rest.split_once('/')?;
    Some(profile)
}

/// Parses and merges a project into one [`File`].
///
/// - files merge in sorted path order;
/// - platform files may only contain `Page` declarations whose base page
///   exists; each becomes a `device.profile == <p>` arm wrapped around the
///   base view (arms sorted by profile id);
/// - duplicate non-override declarations surface through the normal checker.
///
/// # Errors
/// The first parse failure (with the offending path in the message) or an
/// override without a base page.
pub fn merge_project(files: &[SourceFile]) -> Result<File, Diagnostic> {
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|&a, &b| files[a].path.cmp(&files[b].path));

    let mut merged = File { imports: Vec::new(), decls: Vec::new() };
    // Base-page name → index into merged.decls.
    let mut page_index: BTreeMap<String, usize> = BTreeMap::new();
    // Overrides: page name → (profile → view), profiles sorted by BTreeMap.
    let mut overrides: BTreeMap<String, BTreeMap<String, ViewNode>> = BTreeMap::new();

    for &idx in &order {
        let file = &files[idx];
        let parsed = crate::parser::parse_file(&file.source).map_err(|mut diag| {
            diag.message = format!("{}: {}", file.path, diag.message);
            diag
        })?;
        if let Some(profile) = override_profile(&file.path) {
            for decl in parsed.decls {
                match decl {
                    Decl::Page(page) => {
                        overrides
                            .entry(page.name.text.clone())
                            .or_default()
                            .insert(profile.to_string(), page.view);
                    }
                    _ => {
                        return Err(Diagnostic::new(
                            DiagCode::ImportConflict,
                            Span::default(),
                            format!(
                                "{}: platform overrides may only contain Page declarations",
                                file.path
                            ),
                        ));
                    }
                }
            }
        } else {
            merged.imports.extend(parsed.imports);
            for decl in parsed.decls {
                if let Decl::Page(page) = &decl {
                    page_index.insert(page.name.text.clone(), merged.decls.len());
                }
                merged.decls.push(decl);
            }
        }
    }

    // Wrap overridden pages: sorted profiles → deterministic arm order.
    for (page_name, profiles) in overrides {
        let Some(&decl_idx) = page_index.get(&page_name) else {
            return Err(Diagnostic::new(
                DiagCode::UnknownName,
                Span::default(),
                format!("platform override for `{page_name}` has no base page"),
            ));
        };
        let Decl::Page(page) = &mut merged.decls[decl_idx] else { continue };
        let base_view = core::mem::replace(
            &mut page.view,
            ViewNode::If { arms: Vec::new(), els: Vec::new(), span: Span::default() },
        );
        let arms = profiles
            .into_iter()
            .map(|(profile, view)| (profile_cond(&profile), alloc::vec![view]))
            .collect();
        page.view = ViewNode::If { arms, els: alloc::vec![base_view], span: Span::default() };
    }

    Ok(merged)
}

/// `device.profile == <profile>` (synthetic spans — build-generated).
fn profile_cond(profile: &str) -> Expr {
    let span = Span::default();
    Expr::Binary {
        op: crate::ast::BinOp::Eq,
        lhs: alloc::boxed::Box::new(Expr::DeviceRef {
            path: alloc::vec![Ident { text: String::from("profile"), span }],
            span,
        }),
        rhs: alloc::boxed::Box::new(Expr::Path {
            segments: alloc::vec![Ident { text: String::from(profile), span }],
            span,
        }),
        span,
    }
}

/// Canonical project source text for `sourceDigest`: sorted, path-prefixed.
#[must_use]
pub fn canonical_source_set(files: &[SourceFile]) -> String {
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by(|&a, &b| files[a].path.cmp(&files[b].path));
    let mut out = String::new();
    for idx in order {
        out.push_str("=== ");
        out.push_str(&files[idx].path);
        out.push('\n');
        out.push_str(&files[idx].source);
        out.push('\n');
    }
    out
}

/// Compiles an app project directory (`<root>/ui/**.nx`, the
/// docs/dev/dsl/project-layout.md tree) to canonical `.nxir` bytes — the ONE
/// build-time compile path every generator uses (bundlemgrd payload table,
/// windowd demo mount, app-host build), so a payload can never diverge from
/// the CLI's project mode: walk `ui/`, merge, check, lower.
///
/// # Errors
/// A human-readable reason (unreadable tree, parse/check/lower diagnostics) —
/// build scripts fail the BUILD with it (fail-closed, no phantom payloads).
#[cfg(feature = "std")]
pub fn compile_project_dir(root: &std::path::Path) -> Result<alloc::vec::Vec<u8>, String> {
    let ui = root.join("ui");
    let mut files: alloc::vec::Vec<SourceFile> = alloc::vec::Vec::new();
    let mut stack = alloc::vec![ui.clone()];
    while let Some(dir) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| alloc::format!("read {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("nx") {
                let source = std::fs::read_to_string(&path)
                    .map_err(|e| alloc::format!("read {}: {e}", path.display()))?;
                files.push(SourceFile {
                    path: path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/"),
                    source,
                });
            }
        }
    }
    if files.is_empty() {
        return Err(alloc::format!("no .nx sources under {}", ui.display()));
    }
    // Widget libraries (TASK-0081 C3): the app manifest's `dependencies`
    // name SIBLING library folders (`userspace/apps/<lib>/`, bundleType
    // library). Their components are compiled INTO this app's one canonical
    // `.nxir` at build time — no runtime component loading, the
    // one-program-one-hash model and AOT parity stay intact. Governance is
    // enforced HERE: a library contributes ONLY `Component` declarations
    // (compositions of system primitives); anything else fails the build.
    for dep in manifest_dependencies(root)? {
        let lib_root = root
            .parent()
            .map(|parent| parent.join(&dep))
            .filter(|p| p.join("manifest.toml").is_file())
            .ok_or_else(|| alloc::format!("dependency `{dep}`: no sibling app folder"))?;
        let components = lib_root.join("ui/components");
        let entries = std::fs::read_dir(&components).map_err(|e| {
            alloc::format!("dependency `{dep}`: read {}: {e}", components.display())
        })?;
        let mut contributed = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("nx") {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .map_err(|e| alloc::format!("read {}: {e}", path.display()))?;
            let parsed = crate::parse_file(&source)
                .map_err(|d| alloc::format!("dependency `{dep}`: parse: {d:?}"))?;
            if !parsed.decls.iter().all(|decl| matches!(decl, crate::ast::Decl::Component(_))) {
                return Err(alloc::format!(
                    "dependency `{dep}`: {} declares more than components — libraries                      are compositions of system primitives ONLY (TASK-0081 C3)",
                    path.display()
                ));
            }
            files.push(SourceFile {
                path: alloc::format!(
                    "dep:{dep}/{}",
                    path.file_name().and_then(|n| n.to_str()).unwrap_or("component.nx")
                ),
                source,
            });
            contributed += 1;
        }
        if contributed == 0 {
            return Err(alloc::format!("dependency `{dep}`: no components under ui/components"));
        }
    }
    let merged = merge_project(&files).map_err(|d| alloc::format!("merge: {d:?}"))?;
    // Companion surface (TASK-0081 C1): `native/surface.toml` extends the
    // checker's svc table with `svc.<app>.*` for THIS project's check —
    // the app id is the folder name (the manifest SSOT next to it). The
    // guard serializes parallel project checks (global table).
    let _surface_guard = crate::registry::app_surface_guard();
    let surface_path = root.join("native/surface.toml");
    if surface_path.is_file() {
        let app = root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| String::from("project root has no folder name"))?
            .to_string();
        let text = std::fs::read_to_string(&surface_path)
            .map_err(|e| alloc::format!("read {}: {e}", surface_path.display()))?;
        let methods = parse_native_surface(&text)
            .map_err(|e| alloc::format!("{}: {e}", surface_path.display()))?;
        crate::registry::set_app_surface(
            methods.into_iter().map(|(m, arity)| (app.clone(), m, arity)).collect(),
        );
    } else {
        crate::registry::set_app_surface(alloc::vec::Vec::new());
    }
    let (model, diags) = crate::check_file(&merged);
    crate::registry::set_app_surface(alloc::vec::Vec::new());
    if crate::has_errors(&diags) {
        return Err(alloc::format!("check: {diags:?}"));
    }
    let canonical = canonical_source_set(&files);
    // Default-locale catalog (`i18n/en.json`): baked into the program so
    // `@t()` renders real text (keys never leak to the screen). Missing
    // catalog = keys as text (the pre-catalog behavior); a MALFORMED catalog
    // fails the build loudly. Runtime locale switching = TASK-0081 i18n.
    let catalog = load_default_locale_catalog(root)?;
    crate::lower_file_with_catalog(&merged, &model, &canonical, &catalog)
        .map(|lowered| lowered.nxir)
        .map_err(|d| alloc::format!("lower: {d:?}"))
}

/// Loads the app's DEFAULT-locale catalog (`i18n/en.json`, a flat JSON object
/// of `"key": "text"`). Absent file = empty catalog; malformed = build error.
#[cfg(feature = "std")]
fn load_default_locale_catalog(
    root: &std::path::Path,
) -> Result<alloc::collections::BTreeMap<String, String>, String> {
    let path = root.join("i18n/en.json");
    if !path.is_file() {
        return Ok(alloc::collections::BTreeMap::new());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| alloc::format!("read {}: {e}", path.display()))?;
    parse_flat_json_map(&text).map_err(|e| alloc::format!("{}: {e}", path.display()))
}

/// Minimal flat-JSON parser for locale catalogs: ONE object of string→string
/// pairs (the catalog contract). Escapes: `\"`, `\\`, `\n`, `\t`. Anything
/// else — nesting, arrays, numbers, exotic escapes — is a loud error: the
/// catalog format is deliberately this small (no serde in the compiler core).
#[cfg(feature = "std")]
fn parse_flat_json_map(text: &str) -> Result<alloc::collections::BTreeMap<String, String>, String> {
    fn parse_string(
        chars: &mut core::iter::Peekable<core::str::Chars<'_>>,
    ) -> Result<String, String> {
        let mut out = String::new();
        loop {
            match chars.next() {
                Some('"') => return Ok(out),
                Some('\\') => match chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    other => return Err(alloc::format!("unsupported escape {other:?}")),
                },
                Some(c) => out.push(c),
                None => return Err(String::from("unterminated string")),
            }
        }
    }
    let mut map = alloc::collections::BTreeMap::new();
    let mut chars = text.chars().peekable();
    let mut expect = "{";
    loop {
        // Skip whitespace between tokens.
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
        match (expect, chars.next()) {
            ("{", Some('{')) => expect = "key-or-end",
            ("key-or-end", Some('}')) | ("key-or-comma-end", Some('}')) => break,
            ("key-or-end", Some('"')) | ("key", Some('"')) => {
                let key = parse_string(&mut chars)?;
                while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
                    chars.next();
                }
                if chars.next() != Some(':') {
                    return Err(alloc::format!("expected ':' after key {key:?}"));
                }
                while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
                    chars.next();
                }
                if chars.next() != Some('"') {
                    return Err(alloc::format!("expected string value for key {key:?}"));
                }
                let value = parse_string(&mut chars)?;
                map.insert(key, value);
                expect = "key-or-comma-end";
            }
            ("key-or-comma-end", Some(',')) => expect = "key",
            (state, token) => {
                return Err(alloc::format!("unexpected {token:?} (expected {state})"));
            }
        }
    }
    Ok(map)
}

/// Reads the app manifest's `dependencies = ["lib", "lib@^1.0", …]` names
/// (the version constraint after `@` is nxb-pack's concern; the build
/// resolver only needs the folder name). Missing manifest = no deps.
#[cfg(feature = "std")]
fn manifest_dependencies(root: &std::path::Path) -> Result<alloc::vec::Vec<String>, String> {
    let manifest = root.join("manifest.toml");
    if !manifest.is_file() {
        return Ok(alloc::vec::Vec::new());
    }
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| alloc::format!("read {}: {e}", manifest.display()))?;
    let mut deps = alloc::vec::Vec::new();
    let mut in_deps = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let starts = line.starts_with("dependencies") && line.contains('=');
        if starts || in_deps {
            in_deps = true;
            let mut chunks = line.split('"');
            while let (Some(_), Some(value)) = (chunks.next(), chunks.next()) {
                let name = value.split('@').next().unwrap_or(value);
                if !name.is_empty() {
                    deps.push(String::from(name));
                }
            }
            if line.ends_with(']') {
                in_deps = false;
            }
        }
    }
    Ok(deps)
}

/// Parses a companion `native/surface.toml` (TASK-0081 C1) — the ONE place
/// a developer declares the app's native service surface. Line-based on
/// purpose (no TOML dependency in the checker core); the accepted shape is
/// exactly what `nx-dsl add native` scaffolds:
///
/// ```toml
/// [[method]]
/// name = "transcode"
/// args = ["Str", "Int"]
/// result = "Str"
/// ```
///
/// Returns `(method name, positional arity)` entries.
///
/// # Errors
/// A human-readable reason for malformed declarations (fail-closed: a
/// broken surface file fails the BUILD, it never silently shrinks the
/// checker surface).
#[cfg(feature = "std")]
pub fn parse_native_surface(text: &str) -> Result<alloc::vec::Vec<(String, usize)>, String> {
    let mut out: alloc::vec::Vec<(String, usize)> = alloc::vec::Vec::new();
    let mut current: Option<(Option<String>, Option<usize>)> = None;
    let mut flush = |current: &mut Option<(Option<String>, Option<usize>)>| -> Result<(), String> {
        if let Some((name, arity)) = current.take() {
            let name = name.ok_or("`[[method]]` without `name`")?;
            let arity = arity.ok_or_else(|| alloc::format!("method `{name}` without `args`"))?;
            out.push((name, arity));
        }
        Ok(())
    };
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[method]]" {
            flush(&mut current)?;
            current = Some((None, None));
            continue;
        }
        let Some((entry_name, entry_arity)) = current.as_mut() else {
            return Err(alloc::format!("line {}: entry outside `[[method]]`", number + 1));
        };
        if let Some(rest) = line.strip_prefix("name") {
            let value = rest.trim_start().strip_prefix('=').map(str::trim).unwrap_or("");
            let value = value.trim_matches('"');
            if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(alloc::format!("line {}: bad method name", number + 1));
            }
            *entry_name = Some(String::from(value));
        } else if let Some(rest) = line.strip_prefix("args") {
            let value = rest.trim_start().strip_prefix('=').map(str::trim).unwrap_or("");
            if !value.starts_with('[') || !value.ends_with(']') {
                return Err(alloc::format!("line {}: `args` must be a one-line array", number + 1));
            }
            *entry_arity = Some(value.matches('"').count() / 2);
        } else if line.starts_with("result") {
            // Result type recorded for codegen later; the checker only needs arity.
        } else {
            return Err(alloc::format!("line {}: unknown key", number + 1));
        }
    }
    flush(&mut current)?;
    Ok(out)
}

#[cfg(all(test, feature = "std"))]
mod i18n_catalog_tests {
    use super::*;

    fn temp_app(tag: &str, with_catalog: bool) -> std::path::PathBuf {
        let root = std::env::temp_dir()
            .join(format!("nx-i18n-{tag}-{}", std::process::id()))
            .join("myapp");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ui/pages")).expect("mkdir");
        std::fs::write(
            root.join("ui/pages/Main.nx"),
            "Page Main { Stack { Text(@t(\"greeter.title\")) } }\n",
        )
        .expect("write page");
        if with_catalog {
            std::fs::create_dir_all(root.join("i18n")).expect("mkdir i18n");
            std::fs::write(root.join("i18n/en.json"), "{\n  \"greeter.title\": \"Sign in\"\n}\n")
                .expect("write catalog");
        }
        root
    }

    /// The default-locale catalog is BAKED: the compiled program's symbol
    /// table carries the translation, and the raw key no longer needs to be
    /// what the i18n entry resolves to.
    #[test]
    fn default_locale_catalog_bakes_display_text() {
        let root = temp_app("baked", true);
        let nxir = compile_project_dir(&root).expect("compiles with catalog");
        // capnp text fields are raw UTF-8 in the canonical bytes.
        assert!(
            nxir.windows(b"Sign in".len()).any(|w| w == b"Sign in"),
            "translated text must be in the program bytes"
        );
    }

    /// No catalog = the key renders as its own text (pre-catalog behavior).
    #[test]
    fn missing_catalog_keeps_keys_as_text() {
        let root = temp_app("keys", false);
        let nxir = compile_project_dir(&root).expect("compiles without catalog");
        assert!(nxir.windows(b"greeter.title".len()).any(|w| w == b"greeter.title"));
    }

    /// A malformed catalog is a LOUD build error, never silently ignored.
    #[test]
    fn malformed_catalog_fails_the_build() {
        let root = temp_app("bad", true);
        std::fs::write(root.join("i18n/en.json"), "{ nope }").expect("write bad catalog");
        let err = compile_project_dir(&root).expect_err("must fail");
        assert!(err.contains("en.json"), "error names the file: {err}");
    }

    #[test]
    fn flat_json_parser_handles_escapes_and_whitespace() {
        let map = parse_flat_json_map("{ \"a.b\" : \"x \\\" y\", \n  \"c\": \"line\\nbreak\" }")
            .expect("parses");
        assert_eq!(map.get("a.b").map(String::as_str), Some("x \" y"));
        assert_eq!(map.get("c").map(String::as_str), Some("line\nbreak"));
    }
}

#[cfg(all(test, feature = "std"))]
mod native_surface_tests {
    use super::*;

    fn temp_app(tag: &str, with_surface: bool) -> std::path::PathBuf {
        let root = std::env::temp_dir()
            .join(format!("nx-native-{tag}-{}", std::process::id()))
            .join("myapp");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ui/pages")).expect("mkdir");
        std::fs::write(
            root.join("ui/pages/Main.nx"),
            r#"Store S { last: Str = "", busy: Bool = false, }
Event E { Kick, Done(Str), Failed(Int), }
reduce E {
    Kick => state.busy = true,
    Done(text) => { state.last = text; state.busy = false; },
    Failed(code) => state.busy = false,
}
@effect on Kick {
    match svc.myapp.transcode(state.last, timeoutMs: 500) {
        Ok(text) => dispatch(Done(text)),
        Err(e) => dispatch(Failed(e)),
    }
}
Page Main { Stack { Text($state.last) } }
"#,
        )
        .expect("write page");
        if with_surface {
            std::fs::create_dir_all(root.join("native")).expect("mkdir native");
            std::fs::write(
                root.join("native/surface.toml"),
                "[[method]]\nname = \"transcode\"\nargs = [\"Str\"]\nresult = \"Str\"\n",
            )
            .expect("write surface");
        }
        root
    }

    #[test]
    fn companion_surface_extends_the_checker_for_one_project() {
        // WITH the declared surface: svc.myapp.transcode(1 arg) compiles.
        let root = temp_app("ok", true);
        compile_project_dir(&root).expect("compiles with declared surface");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());

        // WITHOUT native/surface.toml: unknown service, fail-closed.
        let root = temp_app("missing", false);
        let err = compile_project_dir(&root).expect_err("must fail without surface");
        assert!(err.contains("UnknownService") || err.contains("NX0207"), "got: {err}");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn surface_toml_parses_and_rejects_malformed() {
        let ok = parse_native_surface(
            "# c\n[[method]]\nname = \"a\"\nargs = []\n[[method]]\nname = \"b\"\nargs = [\"Str\", \"Int\"]\nresult = \"Str\"\n",
        )
        .expect("parses");
        assert_eq!(ok, vec![("a".to_string(), 0), ("b".to_string(), 2)]);
        assert!(parse_native_surface("name = \"x\"\n").is_err(), "entry outside method");
        assert!(parse_native_surface("[[method]]\nargs = []\n").is_err(), "missing name");
        assert!(parse_native_surface("[[method]]\nname = \"x\"\n").is_err(), "missing args");
    }
}

#[cfg(all(test, feature = "std"))]
mod widget_library_tests {
    use super::*;

    fn write(path: &std::path::Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }

    fn apps_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("nx-widgetlib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn scaffold(root: &std::path::Path, lib_component: &str) {
        write(&root.join("app/manifest.toml"), "name = \"app\"\ndependencies = [\"widgets\"]\n");
        write(
            &root.join("app/ui/pages/Main.nx"),
            "Store S { n: Int = 0, }\nEvent E { Tick, }\nreduce E { Tick => state.n += 1, }\nPage Main { Stack { FancyCard { } } }\n",
        );
        write(
            &root.join("widgets/manifest.toml"),
            "name = \"widgets\"\nbundle_type = \"library\"\n",
        );
        write(&root.join("widgets/ui/components/FancyCard.nx"), lib_component);
    }

    #[test]
    fn library_components_compile_into_one_deterministic_nxir() {
        let root = apps_root("ok");
        scaffold(&root, "Component FancyCard {\n    Card { Text(\"lib\") }\n}\n");
        let first = compile_project_dir(&root.join("app")).expect("compiles with lib");
        let second = compile_project_dir(&root.join("app")).expect("recompiles");
        assert_eq!(first, second, "byte-deterministic with resolved library");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn library_declaring_more_than_components_fails_closed() {
        let root = apps_root("gov");
        scaffold(
            &root,
            "Component FancyCard {\n    Card { Text(\"lib\") }\n}\nPage Sneaky { Stack { Text(\"no\") } }\n",
        );
        let err = compile_project_dir(&root.join("app")).expect_err("must refuse");
        assert!(err.contains("more than components"), "got: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_dependency_folder_fails_closed() {
        let root = apps_root("missing");
        scaffold(&root, "Component FancyCard {\n    Card { Text(\"lib\") }\n}\n");
        let _ = std::fs::remove_dir_all(root.join("widgets"));
        let err = compile_project_dir(&root.join("app")).expect_err("must refuse");
        assert!(err.contains("no sibling app folder"), "got: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
