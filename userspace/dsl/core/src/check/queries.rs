// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Query-shape checks (RFC-0076 QuerySpec v1): the declaration form
//! (`Query Name on source { params, where, orderBy, limit }`) and the
//! execution site (`QueryName(arg: v, token: t)`). Queries are pure bounded
//! values — predicate values are literals or declared params, ranges ride the
//! `orderBy` column's index, and `limit` is capped here as well as at the
//! service edge.

use crate::ast::Expr;
use crate::diag::{DiagCode, Diagnostic, Span};
use alloc::{collections::BTreeMap, format, string::String, vec::Vec};

/// Bound on `limit` (the per-page budget; the service re-caps at its edge).
const MAX_QUERY_LIMIT: i64 = 1000;

pub(super) fn check_query_decl(query: &crate::ast::QueryDecl, diags: &mut Vec<Diagnostic>) {
    let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
    for param in &query.params {
        if seen.insert(&param.name.text, ()).is_some() {
            diags.push(Diagnostic::new(
                DiagCode::DuplicateDefinition,
                param.name.span,
                format!("query param `{}` is defined twice", param.name.text),
            ));
        }
        if !matches!(param.ty.name.text.as_str(), "Bool" | "Int" | "Fx" | "Str") {
            diags.push(Diagnostic::new(
                DiagCode::UnknownType,
                param.ty.span,
                format!("query params are scalar (Bool/Int/Fx/Str), not `{}`", param.ty.name.text),
            ));
        }
    }
    for pred in &query.preds {
        match pred.op {
            crate::ast::BinOp::Eq => {}
            crate::ast::BinOp::Ge | crate::ast::BinOp::Le => {
                // v1 rule: ranges ride the order column's index.
                if pred.col.text != query.order_col.text {
                    diags.push(Diagnostic::new(
                        DiagCode::QueryShape,
                        pred.span,
                        format!(
                            "range predicates target the `orderBy` column (`{}`), not `{}`",
                            query.order_col.text, pred.col.text
                        ),
                    ));
                }
            }
            _ => diags.push(Diagnostic::new(
                DiagCode::QueryShape,
                pred.span,
                String::from(
                    "v1 comparisons are `==`, `>=`, `<=` (strict bounds land with the v2 builder)",
                ),
            )),
        }
        let is_param_ref = matches!(
            &pred.value,
            Expr::Path { segments, .. }
                if segments.len() == 1
                    && query.params.iter().any(|p| p.name.text == segments[0].text)
        );
        let is_const = matches!(
            &pred.value,
            Expr::Bool { .. } | Expr::Int { .. } | Expr::Fx { .. } | Expr::Str { .. }
        );
        if !is_param_ref && !is_const {
            diags.push(Diagnostic::new(
                DiagCode::QueryShape,
                pred.value.span(),
                String::from(
                    "predicate values are literals or query params (queries are pure values)",
                ),
            ));
        }
    }
    if query.limit <= 0 || query.limit > MAX_QUERY_LIMIT {
        diags.push(Diagnostic::new(
            DiagCode::QueryShape,
            query.limit_span,
            format!("`limit` must be 1..={MAX_QUERY_LIMIT}"),
        ));
    }
}

/// Validates a `QueryName(args…, token: t)` execution site: every declared
/// param passed exactly once by name; `token:` optional; nothing extra.
pub(super) fn check_query_call(
    query: &crate::ast::QueryDecl,
    args: &[crate::ast::CallArg],
    span: Span,
    diags: &mut Vec<Diagnostic>,
) {
    let mut covered: Vec<bool> = alloc::vec![false; query.params.len()];
    for arg in args {
        let Some(name) = &arg.name else {
            diags.push(Diagnostic::new(
                DiagCode::WrongArity,
                arg.value.span(),
                String::from("query arguments are named (`param: value`)"),
            ));
            continue;
        };
        if name.text == "token" {
            continue;
        }
        match query.params.iter().position(|p| p.name.text == name.text) {
            Some(idx) if covered[idx] => diags.push(Diagnostic::new(
                DiagCode::WrongArity,
                name.span,
                format!("query param `{}` is passed twice", name.text),
            )),
            Some(idx) => covered[idx] = true,
            None => diags.push(Diagnostic::new(
                DiagCode::UnknownField,
                name.span,
                format!("`{}` has no param `{}`", query.name.text, name.text),
            )),
        }
    }
    if covered.iter().any(|&c| !c) {
        let missing: Vec<&str> = query
            .params
            .iter()
            .zip(&covered)
            .filter(|(_, &c)| !c)
            .map(|(p, _)| p.name.text.as_str())
            .collect();
        diags.push(Diagnostic::new(
            DiagCode::WrongArity,
            span,
            format!("`{}` misses params: {}", query.name.text, missing.join(", ")),
        ));
    }
}
