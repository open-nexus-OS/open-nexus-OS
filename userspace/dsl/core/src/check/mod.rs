// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Semantic checking: name resolution + structural rules + lints.
//!
//! Unlike the fail-fast parser, the checker **collects** diagnostics so one
//! run reports everything. `check_file` returns the resolved [`Model`] (used
//! by the lowering pass) plus all diagnostics; callers gate on
//! `has_errors(&diags)` (warnings pass unless `--deny-warn`).

mod lints;
mod names;

use crate::ast::{
    ComponentDecl, EventDecl, File, PageDecl, QueryDecl, ReduceDecl, Route, StoreDecl, WindowDecl,
};
use crate::diag::{Diagnostic, Severity};
use alloc::{collections::BTreeMap, string::String, vec::Vec};

/// Resolved program model — borrowed views into the AST, in declaration
/// order, with deterministic name→index maps.
pub struct Model<'a> {
    pub stores: Vec<&'a StoreDecl>,
    pub events: Vec<&'a EventDecl>,
    pub reduces: Vec<&'a ReduceDecl>,
    pub effects: Vec<&'a crate::ast::EffectDecl>,
    pub pages: Vec<&'a PageDecl>,
    pub components: Vec<&'a ComponentDecl>,
    pub routes: Vec<&'a Route>,
    pub queries: Vec<&'a QueryDecl>,
    pub store_by_name: BTreeMap<&'a str, usize>,
    pub event_by_name: BTreeMap<&'a str, usize>,
    pub page_by_name: BTreeMap<&'a str, usize>,
    pub component_by_name: BTreeMap<&'a str, usize>,
    pub query_by_name: BTreeMap<&'a str, usize>,
    /// Event case name → (event index, case index). Ambiguous names map to
    /// `usize::MAX` markers and are reported.
    pub case_lookup: BTreeMap<&'a str, (usize, usize)>,
    /// All `@t("…")` keys in reference order (deduped, sorted at lowering).
    pub i18n_keys: Vec<String>,
    /// The app-owned window intent (`Window { … }`), if declared. At most one
    /// per program (a duplicate is a diagnostic). `None` = frame defaults.
    pub window: Option<&'a WindowDecl>,
}

/// Checks one file (v0.1: single-file programs; multi-file merge lands with
/// the module-resolution increment).
pub fn check_file(file: &File) -> (Model<'_>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let model = names::build_model(file, &mut diags);
    names::check_references(file, &model, &mut diags);
    lints::run(file, &model, &mut diags);
    (model, diags)
}

/// True if any diagnostic is a hard error.
#[must_use]
pub fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity() == Severity::Error)
}
