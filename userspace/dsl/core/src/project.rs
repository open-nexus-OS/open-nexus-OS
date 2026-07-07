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
        page.view =
            ViewNode::If { arms, els: alloc::vec![base_view], span: Span::default() };
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
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| alloc::format!("read {}: {e}", dir.display()))?;
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
    let merged = merge_project(&files).map_err(|d| alloc::format!("merge: {d:?}"))?;
    let (model, diags) = crate::check_file(&merged);
    if crate::has_errors(&diags) {
        return Err(alloc::format!("check: {diags:?}"));
    }
    let canonical = canonical_source_set(&files);
    crate::lower_file(&merged, &model, &canonical)
        .map(|lowered| lowered.nxir)
        .map_err(|d| alloc::format!("lower: {d:?}"))
}
