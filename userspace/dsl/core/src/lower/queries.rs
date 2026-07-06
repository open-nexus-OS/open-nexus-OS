// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: QuerySpec lowering — `Query` declarations → the IR's
//! `querySpecs` table (v1.3), and the shared helper the effect walker uses
//! to turn a `match QueryName(args…) { Ok/Err }` into a `QueryStep`.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: conformance corpus (query cases) + host suite

use super::exprs::{lower_expr, Env};
use super::{unsupported, Ctx};
use crate::ast::{BinOp, Expr, QueryDecl};
use crate::check::Model;
use crate::diag::Diagnostic;
use nexus_dsl_ir::ui_ir_capnp as ir;

/// Emits the canonical (name-sorted) `querySpecs` list.
pub(super) fn build_query_specs(
    ctx: &Ctx<'_>,
    model: &Model<'_>,
    program: &mut ir::ui_program::Builder<'_>,
) -> Result<(), Diagnostic> {
    let mut specs = program.reborrow().init_query_specs(ctx.query_order.len() as u32);
    for (canonical, &model_idx) in ctx.query_order.iter().enumerate() {
        let query = model.queries[model_idx];
        let mut spec = specs.reborrow().get(canonical as u32);
        spec.set_source(ctx.sym(&query.source.text));
        spec.set_param_count(query.params.len() as u16);
        spec.set_order_col(ctx.sym(&query.order_col.text));
        spec.set_descending(query.descending);
        spec.set_limit(query.limit.max(0) as u32);
        let mut preds = spec.init_preds(query.preds.len() as u32);
        for (i, pred) in query.preds.iter().enumerate() {
            let mut p = preds.reborrow().get(i as u32);
            p.set_col(ctx.sym(&pred.col.text));
            p.set_op(match pred.op {
                BinOp::Eq => ir::QueryOp::Eq,
                BinOp::Ge => ir::QueryOp::Ge,
                BinOp::Le => ir::QueryOp::Le,
                // The checker rejects strict ops (NX0410) before lowering;
                // stay fail-closed if it ever slips through.
                _ => return Err(unsupported(pred.span, "strict comparisons in a query")),
            });
            lower_pred_value(query, &pred.value, p.init_value())?;
        }
    }
    Ok(())
}

/// Predicate values are const literals or param references (`paramGet` by
/// declaration position) — checked as NX0410 before lowering.
fn lower_pred_value(
    query: &QueryDecl,
    value: &Expr,
    builder: ir::expr::Builder<'_>,
) -> Result<(), Diagnostic> {
    let mut b = builder;
    match value {
        Expr::Bool { value, .. } => {
            b.reborrow().init_type().set_bool(());
            b.set_lit_bool(*value);
        }
        Expr::Int { value, .. } => {
            b.reborrow().init_type().set_int(());
            b.set_lit_int(*value);
        }
        Expr::Fx { value, .. } => {
            b.reborrow().init_type().set_fx(());
            b.set_lit_fx(*value);
        }
        Expr::Str { value, .. } => {
            b.reborrow().init_type().set_str(0);
            b.set_lit_str(capnp::text::Reader::from(value.as_str()));
        }
        Expr::Path { segments, span } if segments.len() == 1 => {
            let Some(idx) =
                query.params.iter().position(|p| p.name.text == segments[0].text)
            else {
                return Err(unsupported(*span, "a non-param name as a predicate value"));
            };
            b.reborrow().init_type().set_opaque(());
            b.set_param_get(idx as u32);
        }
        other => {
            return Err(unsupported(other.span(), "a computed predicate value"));
        }
    }
    Ok(())
}

/// Lowers a `match QueryName(args…, token: t) { Ok(rows, next) => dispatch…,
/// Err(e) => dispatch…, }` scrutinee into a `QueryStep`. `args` are reordered
/// to declaration order (they are named; the checker verified coverage).
pub(super) fn fill_query_step(
    ctx: &Ctx<'_>,
    env: &mut Env<'_>,
    query_canonical: u32,
    query: &QueryDecl,
    args: &[crate::ast::CallArg],
    span: crate::diag::Span,
    step: &mut ir::query_step::Builder<'_>,
) -> Result<(), Diagnostic> {
    step.set_spec(query_canonical);
    {
        let mut list = step.reborrow().init_args(query.params.len() as u32);
        for (i, param) in query.params.iter().enumerate() {
            let Some(arg) = args.iter().find(|a| {
                a.name.as_ref().map(|n| n.text.as_str()) == Some(param.name.text.as_str())
            }) else {
                return Err(unsupported(span, "a query call missing a declared param"));
            };
            lower_expr(env, &arg.value, list.reborrow().get(i as u32))?;
        }
    }
    {
        let token_builder = step.reborrow().init_token();
        match args
            .iter()
            .find(|a| a.name.as_ref().map(|n| n.text.as_str()) == Some("token"))
        {
            Some(arg) => lower_expr(env, &arg.value, token_builder)?,
            None => {
                // Default: first page.
                let mut b = token_builder;
                b.reborrow().init_type().set_str(0);
                b.set_lit_str(capnp::text::Reader::from(""));
            }
        }
    }
    let _ = ctx;
    Ok(())
}
