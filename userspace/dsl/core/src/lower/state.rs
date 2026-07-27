// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The program's state tables: stores (named + component-owned implicit),
//! event types with their cases, reducer arms in event-case order, and
//! `@effect` plans. Everything here is emitted in the canonical order the
//! `Ctx` computed — source order never leaks into the IR.

use super::effects::lower_effect_steps;
use super::exprs::{lower_expr, lower_stmts, lower_type, Env};
use super::{unsupported, Ctx};
use crate::ast::{Expr, Stmt};
use crate::check::Model;
use crate::diag::Diagnostic;
use alloc::vec::Vec;
use nexus_dsl_ir::ui_ir_capnp as ir;

pub(super) fn build_state(
    ctx: &Ctx<'_>,
    model: &Model<'_>,
    program: &mut ir::ui_program::Builder<'_>,
) -> Result<(), Diagnostic> {
    // Stores (canonical order): named stores, then component-local implicit
    // stores (`state:` blocks) in component-name order.
    {
        let total = ctx.store_order.len() + ctx.local_stores.len();
        let mut stores = program.reborrow().init_stores(total as u32);
        for (i, &model_idx) in ctx.store_order.iter().enumerate() {
            let store = model.stores[model_idx];
            let mut b = stores.reborrow().get(i as u32);
            b.set_name(ctx.sym(&store.name.text));
            let mut fields = b.init_fields(store.fields.len() as u32);
            for (j, field) in store.fields.iter().enumerate() {
                let mut fb = fields.reborrow().get(j as u32);
                fb.set_name(ctx.sym(&field.name.text));
                fb.set_persist(field.persist);
                lower_type(&field.ty, fb.reborrow().init_type());
                if let Some(default) = &field.default {
                    let env = Env::new(ctx);
                    lower_expr(&env, default, fb.init_default())?;
                }
            }
        }
        for (offset, (component_name, _, comp_idx)) in ctx.local_stores.iter().enumerate() {
            let component = model.components[*comp_idx];
            let mut b = stores.reborrow().get((ctx.store_order.len() + offset) as u32);
            b.set_name(ctx.sym(&alloc::format!("__local_{component_name}")));
            let mut fields = b.init_fields(component.state.len() as u32);
            for (j, field) in component.state.iter().enumerate() {
                let mut fb = fields.reborrow().get(j as u32);
                fb.set_name(ctx.sym(&field.name.text));
                fb.set_persist(false);
                lower_type(&field.ty, fb.reborrow().init_type());
                if let Some(default) = &field.default {
                    let env = Env::new(ctx);
                    lower_expr(&env, default, fb.init_default())?;
                }
            }
        }
    }

    // Events (canonical order).
    {
        let mut events = program.reborrow().init_events(ctx.event_order.len() as u32);
        for (i, &model_idx) in ctx.event_order.iter().enumerate() {
            let event = model.events[model_idx];
            let mut b = events.reborrow().get(i as u32);
            b.set_name(ctx.sym(&event.name.text));
            let mut cases = b.init_cases(event.cases.len() as u32);
            for (j, case) in event.cases.iter().enumerate() {
                let mut cb = cases.reborrow().get(j as u32);
                cb.set_name(ctx.sym(&case.name.text));
                let mut payload = cb.init_payload(case.payload.len() as u32);
                for (k, ty) in case.payload.iter().enumerate() {
                    lower_type(ty, payload.reborrow().get(k as u32));
                }
            }
        }
    }

    // Reducers: each binds ONE store, resolved from the state fields its
    // arms touch (assignments + reads). Cross-store updates are separate
    // reducers listening to the same event — dispatch runs them all.
    {
        let mut reducers = program.reborrow().init_reducers(model.reduces.len() as u32);
        // Canonical order: by event canonical index.
        let mut order: Vec<usize> = (0..model.reduces.len()).collect();
        order.sort_by_key(|&i| {
            ctx.event_index.get(model.reduces[i].event.text.as_str()).copied().unwrap_or(0)
        });
        for (i, &model_idx) in order.iter().enumerate() {
            let reduce = model.reduces[model_idx];
            let bound_store = resolve_reducer_store(ctx, reduce)?;
            let mut b = reducers.reborrow().get(i as u32);
            b.set_store(bound_store);
            b.set_event(ctx.event_index.get(reduce.event.text.as_str()).copied().unwrap_or(0));
            let mut arms_sorted: Vec<&crate::ast::ReduceArm> = reduce.arms.iter().collect();
            arms_sorted.sort_by_key(|arm| {
                ctx.event_case(arm.pattern.case.text.as_str()).map(|(_, c)| c).unwrap_or(0)
            });
            let mut arms = b.init_arms(arms_sorted.len() as u32);
            for (j, arm) in arms_sorted.iter().enumerate() {
                let mut ab = arms.reborrow().get(j as u32);
                ab.set_case(
                    ctx.event_case(arm.pattern.case.text.as_str()).map(|(_, c)| c).unwrap_or(0),
                );
                let mut env = Env::new(ctx);
                {
                    let slots: Vec<u32> =
                        arm.pattern.binds.iter().map(|bind| env.bind_local(&bind.text)).collect();
                    let mut binds = ab.reborrow().init_binds(slots.len() as u32);
                    for (k, slot) in slots.iter().enumerate() {
                        binds.set(k as u32, *slot);
                    }
                }
                lower_stmts(&mut env, &arm.body, ab.init_body(arm.body.len() as u32))?;
            }
        }
    }

    // Effects: linear plans.
    {
        let mut order: Vec<usize> = (0..model.effects.len()).collect();
        order.sort_by_key(|&i| {
            ctx.event_case(model.effects[i].trigger.case.text.as_str()).unwrap_or((0, 0))
        });
        let mut effects = program.reborrow().init_effects(model.effects.len() as u32);
        for (i, &model_idx) in order.iter().enumerate() {
            let effect = model.effects[model_idx];
            let mut b = effects.reborrow().get(i as u32);
            let (event_idx, case_idx) =
                ctx.event_case(effect.trigger.case.text.as_str()).unwrap_or((0, 0));
            b.set_event(event_idx);
            b.set_case(case_idx);
            let mut env = Env::new(ctx);
            {
                let slots: Vec<u32> =
                    effect.trigger.binds.iter().map(|bind| env.bind_local(&bind.text)).collect();
                let mut binds = b.reborrow().init_binds(slots.len() as u32);
                for (k, slot) in slots.iter().enumerate() {
                    binds.set(k as u32, *slot);
                }
            }
            lower_effect_steps(ctx, &mut env, &effect.body, &mut b)?;
        }
    }
    Ok(())
}

/// Resolves the single store a reducer's arms touch via `state.<field>`
/// paths (assign targets and reads). One reducer = one store; mixing is a
/// lowering error (write two reducers on the same event instead).
fn resolve_reducer_store(
    ctx: &Ctx<'_>,
    reduce: &crate::ast::ReduceDecl,
) -> Result<u32, Diagnostic> {
    fn walk_expr(ctx: &Ctx<'_>, expr: &Expr, found: &mut Option<u32>) -> Result<(), Diagnostic> {
        match expr {
            Expr::StateRef { path, span } => {
                if let Some(first) = path.first() {
                    match ctx.store_of_field(&first.text) {
                        Ok(store) => match found {
                            Some(existing) if *existing != store => {
                                return Err(unsupported(
                                    *span,
                                    "one reducer touching two stores (split it)",
                                ));
                            }
                            _ => *found = Some(store),
                        },
                        Err(_) => {
                            return Err(unsupported(*span, "an unresolvable state field"));
                        }
                    }
                }
                Ok(())
            }
            Expr::Unary { operand, .. } => walk_expr(ctx, operand, found),
            Expr::Binary { lhs, rhs, .. } => {
                walk_expr(ctx, lhs, found)?;
                walk_expr(ctx, rhs, found)
            }
            Expr::List { items, .. } | Expr::EnumLit { args: items, .. } => {
                for item in items {
                    walk_expr(ctx, item, found)?;
                }
                Ok(())
            }
            Expr::I18n { args, .. } => {
                for arg in args {
                    walk_expr(ctx, arg, found)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn walk_stmts(
        ctx: &Ctx<'_>,
        stmts: &[Stmt],
        found: &mut Option<u32>,
    ) -> Result<(), Diagnostic> {
        for stmt in stmts {
            match stmt {
                Stmt::Assign { path, value, span, .. } => {
                    if let Some(first) = path.first() {
                        match ctx.store_of_field(&first.text) {
                            Ok(store) => match found {
                                Some(existing) if *existing != store => {
                                    return Err(unsupported(
                                        *span,
                                        "one reducer touching two stores (split it)",
                                    ));
                                }
                                _ => *found = Some(store),
                            },
                            Err(_) => {
                                return Err(unsupported(*span, "an unresolvable state field"));
                            }
                        }
                    }
                    walk_expr(ctx, value, found)?;
                }
                Stmt::Let { value, .. } => walk_expr(ctx, value, found)?,
                Stmt::If { cond, then, els, .. } => {
                    walk_expr(ctx, cond, found)?;
                    walk_stmts(ctx, then, found)?;
                    walk_stmts(ctx, els, found)?;
                }
                Stmt::Match { scrutinee, arms, .. } => {
                    walk_expr(ctx, scrutinee, found)?;
                    for arm in arms {
                        walk_stmts(ctx, &arm.body, found)?;
                    }
                }
                Stmt::Dispatch { args, .. } => {
                    for arg in args {
                        walk_expr(ctx, arg, found)?;
                    }
                }
                Stmt::ExprStmt { expr, .. } => walk_expr(ctx, expr, found)?,
            }
        }
        Ok(())
    }
    let mut found = None;
    for arm in &reduce.arms {
        walk_stmts(ctx, &arm.body, &mut found)?;
    }
    // A no-op reducer (touches nothing) binds store 0 deterministically.
    Ok(found.unwrap_or(0))
}
