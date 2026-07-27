// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Effect lowering: `@effect` bodies → bounded linear step plans.
//!
//! Effects are NOT imperative code. A body lowers to an ordered, bounded list
//! of steps: a call step binds its result on Ok and continues, dispatches
//! `onErr` and stops on Err; a `match` directly on a call result collapses
//! into that call's onOk/onErr arms. Anything outside that shape reports
//! `NX0501 LoweringUnsupported` — never silently dropped.

use super::exprs::{lower_expr, Env};
use super::{unsupported, Ctx};
use crate::ast::{Expr, Stmt};
use crate::diag::Diagnostic;
use nexus_dsl_ir::ui_ir_capnp as ir;

/// Effect bodies → bounded step lists. Semantics: steps run in order; a call
/// step binds its result on Ok and continues, dispatches `onErr` and stops on
/// Err; a `match` directly on a call becomes the call's onOk/onErr.
pub(super) fn lower_effect_steps(
    ctx: &Ctx<'_>,
    env: &mut Env<'_>,
    body: &[Stmt],
    plan: &mut ir::effect_plan::Builder<'_>,
) -> Result<(), Diagnostic> {
    let mut steps = plan.reborrow().init_steps(body.len() as u32);
    for (i, stmt) in body.iter().enumerate() {
        let step = steps.reborrow().get(i as u32);
        match stmt {
            Stmt::Let { name, value: Expr::Call { path, args, span }, .. } => {
                let slot = env.next_slot; // bound after args lower
                let mut call = step.init_call();
                fill_call(ctx, env, path, args, *span, &mut call)?;
                call.set_result_slot(slot);
                let _ = env.bind_local(&name.text);
            }
            Stmt::ExprStmt { expr: Expr::Call { path, args, span }, .. } => {
                let mut call = step.init_call();
                fill_call(ctx, env, path, args, *span, &mut call)?;
                call.set_result_slot(u32::MAX);
            }
            Stmt::Dispatch { case, args, span } => {
                let mut dispatch = step.init_dispatch();
                fill_dispatch(ctx, env, case, args, *span, &mut dispatch)?;
            }
            Stmt::Match { scrutinee: Expr::Call { path, args, span }, arms, .. }
                if path.len() == 1 && ctx.query_index.contains_key(path[0].text.as_str()) =>
            {
                // `match QueryName(args…, token: t) { Ok(rows, next) => dispatch(..),
                //  Err(e) => dispatch(..), }` — the ONLY query execution site.
                let canonical = ctx.query_index[path[0].text.as_str()];
                let query = ctx.queries[canonical as usize];
                let mut qstep = step.init_query();
                super::queries::fill_query_step(
                    ctx, env, canonical, query, args, *span, &mut qstep,
                )?;
                // Ok binds (rows, next); Err binds the error code into the
                // rows slot — only one path ever runs.
                let rows_slot = env.bind_local("__query_rows");
                let next_slot = env.bind_local("__query_next");
                qstep.set_rows_slot(rows_slot);
                qstep.set_next_slot(next_slot);
                for arm in arms {
                    let is_ok = arm.pattern.case.text == "Ok";
                    let is_err = arm.pattern.case.text == "Err";
                    if !is_ok && !is_err {
                        return Err(unsupported(arm.span, "non-Ok/Err arm on a query result"));
                    }
                    let [Stmt::Dispatch { case, args, span }] = arm.body.as_slice() else {
                        return Err(unsupported(
                            arm.span,
                            "query-result arms beyond a single dispatch",
                        ));
                    };
                    if is_ok {
                        if let Some(bind) = arm.pattern.binds.first() {
                            env.bind_local_to(&bind.text, rows_slot);
                        }
                        if let Some(bind) = arm.pattern.binds.get(1) {
                            env.bind_local_to(&bind.text, next_slot);
                        }
                    } else if let Some(bind) = arm.pattern.binds.first() {
                        env.bind_local_to(&bind.text, rows_slot);
                    }
                    let mut target = if is_ok {
                        qstep.reborrow().init_on_page()
                    } else {
                        qstep.reborrow().init_on_err()
                    };
                    fill_dispatch(ctx, env, case, args, *span, &mut target)?;
                }
            }
            Stmt::Match { scrutinee: Expr::Call { path, args, span }, arms, .. } => {
                // `match svc.x.y(...) { Ok(v) => dispatch(..), Err(e) => dispatch(..), }`
                // Ok and Err arms share ONE result slot: only one path runs
                // (Ok -> the call result, Err -> the stable error code).
                let mut call = step.init_call();
                fill_call(ctx, env, path, args, *span, &mut call)?;
                let shared_slot = env.bind_local("__call_result");
                call.set_result_slot(shared_slot);
                for arm in arms {
                    let is_ok = arm.pattern.case.text == "Ok";
                    let is_err = arm.pattern.case.text == "Err";
                    if !is_ok && !is_err {
                        return Err(unsupported(arm.span, "non-Ok/Err arm on a call result"));
                    }
                    let [Stmt::Dispatch { case, args, span }] = arm.body.as_slice() else {
                        return Err(unsupported(
                            arm.span,
                            "call-result arms beyond a single dispatch",
                        ));
                    };
                    if let Some(bind) = arm.pattern.binds.first() {
                        env.bind_local_to(&bind.text, shared_slot);
                    }
                    let mut target = if is_ok {
                        call.reborrow().init_on_ok()
                    } else {
                        call.reborrow().init_on_err()
                    };
                    fill_dispatch(ctx, env, case, args, *span, &mut target)?;
                }
            }
            other => {
                return Err(unsupported(
                    match other {
                        Stmt::Assign { span, .. }
                        | Stmt::Let { span, .. }
                        | Stmt::If { span, .. }
                        | Stmt::Match { span, .. }
                        | Stmt::Dispatch { span, .. }
                        | Stmt::ExprStmt { span, .. } => *span,
                    },
                    "this statement form in an effect plan",
                ));
            }
        }
    }
    Ok(())
}

fn fill_call(
    ctx: &Ctx<'_>,
    env: &Env<'_>,
    path: &[crate::ast::Ident],
    args: &[crate::ast::CallArg],
    span: crate::diag::Span,
    call: &mut ir::call_step::Builder<'_>,
) -> Result<(), Diagnostic> {
    if path.len() != 3 || path[0].text != "svc" {
        return Err(unsupported(span, "calls other than `svc.<service>.<method>(…)`"));
    }
    call.set_service(ctx.sym(&path[1].text));
    call.set_method(ctx.sym(&path[2].text));
    let mut timeout: u32 = 0;
    let positional: Vec<&crate::ast::CallArg> = args
        .iter()
        .filter(|arg| {
            if arg.name.as_ref().map(|n| n.text.as_str()) == Some("timeoutMs") {
                if let Expr::Int { value, .. } = arg.value {
                    timeout = value.max(0) as u32;
                }
                false
            } else {
                true
            }
        })
        .collect();
    call.set_timeout_ms(timeout);
    let mut list = call.reborrow().init_args(positional.len() as u32);
    for (i, arg) in positional.iter().enumerate() {
        lower_expr(env, &arg.value, list.reborrow().get(i as u32))?;
    }
    Ok(())
}

pub(super) fn fill_dispatch(
    ctx: &Ctx<'_>,
    env: &Env<'_>,
    case: &crate::ast::Ident,
    args: &[Expr],
    span: crate::diag::Span,
    dispatch: &mut ir::dispatch_step::Builder<'_>,
) -> Result<(), Diagnostic> {
    let Some((event_idx, case_idx)) = ctx.event_case(case.text.as_str()) else {
        return Err(unsupported(span, "dispatch to an unresolved case"));
    };
    dispatch.set_event(event_idx);
    dispatch.set_case(case_idx);
    let mut payload = dispatch.reborrow().init_payload(args.len() as u32);
    for (i, arg) in args.iter().enumerate() {
        lower_expr(env, arg, payload.reborrow().get(i as u32))?;
    }
    Ok(())
}
