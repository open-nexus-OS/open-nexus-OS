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
