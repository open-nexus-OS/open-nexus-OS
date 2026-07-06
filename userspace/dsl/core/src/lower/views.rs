// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! State tables (stores/events/reducers/effects), component/view lowering
//! (with persisted NodeIds), and routes.

use super::exprs::{lower_expr, lower_stmts, lower_type, Env};
use super::{unsupported, ComponentSource, Ctx};
use crate::ast::{
    Expr, HandlerAction, ModifierCall, Stmt, ViewNode, WidgetNode,
};
use crate::check::Model;
use crate::diag::Diagnostic;
use crate::registry;
use alloc::vec::Vec;
use nexus_dsl_ir::node_id::static_node_id;
use nexus_dsl_ir::ui_ir_capnp as ir;

// ------------------------------------------------------------------- state

pub(super) fn build_state(
    ctx: &Ctx<'_>,
    model: &Model<'_>,
    program: &mut ir::ui_program::Builder<'_>,
) -> Result<(), Diagnostic> {
    // Stores (canonical order).
    {
        let mut stores = program.reborrow().init_stores(ctx.store_order.len() as u32);
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
                    let env = Env::new(ctx, None);
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

    // Reducers: single-store binding (v0.1); arms sorted by case index.
    let single_store: Option<u32> = if ctx.store_order.len() == 1 { Some(0) } else { None };
    {
        let mut reducers = program.reborrow().init_reducers(model.reduces.len() as u32);
        // Canonical order: by event canonical index.
        let mut order: Vec<usize> = (0..model.reduces.len()).collect();
        order.sort_by_key(|&i| {
            ctx.event_index.get(model.reduces[i].event.text.as_str()).copied().unwrap_or(0)
        });
        for (i, &model_idx) in order.iter().enumerate() {
            let reduce = model.reduces[model_idx];
            if single_store.is_none() {
                return Err(unsupported(reduce.span, "multi-store `reduce` binding"));
            }
            let mut b = reducers.reborrow().get(i as u32);
            b.set_store(0);
            b.set_event(
                ctx.event_index.get(reduce.event.text.as_str()).copied().unwrap_or(0),
            );
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
                let mut env = Env::new(ctx, single_store);
                {
                    let slots: Vec<u32> = arm
                        .pattern
                        .binds
                        .iter()
                        .map(|bind| env.bind_local(&bind.text))
                        .collect();
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
            let mut env = Env::new(ctx, if ctx.store_order.len() == 1 { Some(0) } else { None });
            {
                let slots: Vec<u32> = effect
                    .trigger
                    .binds
                    .iter()
                    .map(|bind| env.bind_local(&bind.text))
                    .collect();
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

/// Effect bodies → bounded step lists. Semantics: steps run in order; a call
/// step binds its result on Ok and continues, dispatches `onErr` and stops on
/// Err; a `match` directly on a call becomes the call's onOk/onErr.
fn lower_effect_steps(
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

fn fill_dispatch(
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

// -------------------------------------------------------------- components

pub(super) fn build_components(
    ctx: &Ctx<'_>,
    model: &Model<'_>,
    program: &mut ir::ui_program::Builder<'_>,
) -> Result<(), Diagnostic> {
    let single_store: Option<u32> = if ctx.store_order.len() == 1 { Some(0) } else { None };
    let mut components =
        program.reborrow().init_components(ctx.component_order.len() as u32);
    for (i, (name, source)) in ctx.component_order.iter().enumerate() {
        let mut b = components.reborrow().get(i as u32);
        b.set_name(ctx.sym(name));
        let mut env = Env::new(ctx, single_store);
        let view = match source {
            ComponentSource::Page(idx) => {
                b.set_is_page(true);
                b.reborrow().init_props(0);
                &model.pages[*idx].view
            }
            ComponentSource::Component(idx) => {
                let component = model.components[*idx];
                b.set_is_page(false);
                let mut props = b.reborrow().init_props(component.props.len() as u32);
                for (j, prop) in component.props.iter().enumerate() {
                    let mut pb = props.reborrow().get(j as u32);
                    pb.set_name(ctx.sym(&prop.name.text));
                    lower_type(&prop.ty, pb.init_type());
                    env.params.insert(prop.name.text.clone(), j as u32);
                }
                &component.view
            }
        };
        let mut path: Vec<u32> = Vec::new();
        lower_view(ctx, &mut env, name, &mut path, view, b.init_view())?;
    }
    Ok(())
}

fn lower_view(
    ctx: &Ctx<'_>,
    env: &mut Env<'_>,
    component: &str,
    path: &mut Vec<u32>,
    node: &ViewNode,
    builder: ir::view_node::Builder<'_>,
) -> Result<(), Diagnostic> {
    let mut b = builder;
    b.set_node_id(static_node_id(component, path));
    match node {
        ViewNode::Widget(widget) => {
            let is_component = ctx.component_index.contains_key(widget.name.text.as_str());
            if is_component {
                lower_component_ref(ctx, env, widget, b)
            } else {
                lower_widget(ctx, env, component, path, widget, b)
            }
        }
        ViewNode::If { arms, els, .. } => {
            let branch = b.init_branch();
            lower_branch(ctx, env, component, path, arms, els, branch)
        }
        ViewNode::Match { scrutinee, arms, span } => {
            // Bind-less match lowers to an equality branch chain.
            let mut cond_arms: Vec<(Expr, Vec<ViewNode>)> = Vec::new();
            for arm in arms {
                if !arm.pattern.binds.is_empty() {
                    return Err(unsupported(*span, "view `match` with payload binds"));
                }
                let cond = Expr::Binary {
                    op: crate::ast::BinOp::Eq,
                    lhs: alloc::boxed::Box::new(scrutinee.clone()),
                    rhs: alloc::boxed::Box::new(Expr::EnumLit {
                        ty: arm.pattern.case.clone(),
                        case: arm.pattern.case.clone(),
                        args: Vec::new(),
                        span: arm.pattern.span,
                    }),
                    span: arm.pattern.span,
                };
                cond_arms.push((cond, arm.body.clone()));
            }
            let branch = b.init_branch();
            lower_branch(ctx, env, component, path, &cond_arms, &[], branch)
        }
        ViewNode::For { var, iter, body, span } => {
            if body.len() != 1 {
                return Err(unsupported(*span, "multi-root `for` templates"));
            }
            let mut fe = b.init_for_each();
            fe.set_windowed(false);
            lower_expr(env, iter, fe.reborrow().init_binding())?;
            let slot = env.bind_local(&var.text);
            fe.set_bind_slot(slot);
            {
                let mut key = fe.reborrow().init_key_expr();
                key.reborrow().init_type().set_int(());
                key.set_lit_int(0); // positional identity for static `for`
            }
            path.push(0);
            lower_view(ctx, env, component, path, &body[0], fe.init_template())?;
            path.pop();
            Ok(())
        }
        ViewNode::Collection(collection) => {
            if collection.body.len() != 1 {
                return Err(unsupported(collection.span, "multi-root collection templates"));
            }
            let mut fe = b.init_for_each();
            fe.set_windowed(true);
            lower_expr(env, &collection.binding, fe.reborrow().init_binding())?;
            let slot = env.bind_local(&collection.var.text);
            fe.set_bind_slot(slot);
            // The template root's `.key(expr)` is the collection key.
            let key_expr = template_key(&collection.body[0]);
            match key_expr {
                Some(expr) => lower_expr(env, expr, fe.reborrow().init_key_expr())?,
                None => {
                    // Checker reported MissingKey; stay total.
                    let mut key = fe.reborrow().init_key_expr();
                    key.reborrow().init_type().set_int(());
                    key.set_lit_int(0);
                }
            }
            path.push(0);
            lower_view(ctx, env, component, path, &collection.body[0], fe.init_template())?;
            path.pop();
            Ok(())
        }
    }
}

fn lower_branch(
    ctx: &Ctx<'_>,
    env: &mut Env<'_>,
    component: &str,
    path: &mut Vec<u32>,
    arms: &[(Expr, Vec<ViewNode>)],
    els: &[ViewNode],
    mut branch: ir::branch::Builder<'_>,
) -> Result<(), Diagnostic> {
    {
        let mut arm_list = branch.reborrow().init_arms(arms.len() as u32);
        for (i, (cond, body)) in arms.iter().enumerate() {
            let mut ab = arm_list.reborrow().get(i as u32);
            lower_expr(env, cond, ab.reborrow().init_cond())?;
            let mut body_list = ab.init_body(body.len() as u32);
            for (j, child) in body.iter().enumerate() {
                path.push((i as u32) << 8 | j as u32);
                lower_view(ctx, env, component, path, child, body_list.reborrow().get(j as u32))?;
                path.pop();
            }
        }
    }
    let mut else_list = branch.init_else_body(els.len() as u32);
    for (j, child) in els.iter().enumerate() {
        path.push(0xff00 | j as u32);
        lower_view(ctx, env, component, path, child, else_list.reborrow().get(j as u32))?;
        path.pop();
    }
    Ok(())
}

fn template_key(node: &ViewNode) -> Option<&Expr> {
    if let ViewNode::Widget(widget) = node {
        widget
            .modifiers
            .iter()
            .find(|m| m.name.text == "key")
            .and_then(|m| m.args.first())
            .map(|arg| &arg.value)
    } else {
        None
    }
}

fn lower_widget(
    ctx: &Ctx<'_>,
    env: &mut Env<'_>,
    component: &str,
    path: &mut Vec<u32>,
    widget: &WidgetNode,
    builder: ir::view_node::Builder<'_>,
) -> Result<(), Diagnostic> {
    let mut w = builder.init_widget();
    w.set_kind(ctx.sym(&widget.name.text));

    // Props: positional sugar resolves to the registry primary prop; the
    // final list is name-sorted (canonical).
    let primary = registry::widget_spec(&widget.name.text).and_then(|s| s.primary_prop);
    let mut props: Vec<(&str, &Expr)> =
        widget.props.iter().map(|(name, value)| (name.text.as_str(), value)).collect();
    if let (Some(positional), Some(primary)) = (&widget.positional, primary) {
        props.push((primary, positional));
    } else if widget.positional.is_some() {
        return Err(unsupported(widget.span, "positional argument on this node"));
    }
    props.sort_by_key(|(name, _)| *name);
    {
        let mut list = w.reborrow().init_props(props.len() as u32);
        for (i, (name, value)) in props.iter().enumerate() {
            let mut pb = list.reborrow().get(i as u32);
            pb.set_name(ctx.sym(name));
            lower_expr(env, value, pb.init_value())?;
        }
    }

    lower_modifiers(ctx, env, &widget.modifiers, &mut w)?;

    // Handlers.
    {
        let mut handlers = w.reborrow().init_handlers(widget.handlers.len() as u32);
        for (i, handler) in widget.handlers.iter().enumerate() {
            let mut hb = handlers.reborrow().get(i as u32);
            hb.set_trigger(ctx.sym(&handler.trigger.text));
            match &handler.action {
                HandlerAction::Dispatch { case, args } => {
                    let mut dispatch = hb.init_dispatch();
                    fill_dispatch(ctx, env, case, args, handler.span, &mut dispatch)?;
                }
                HandlerAction::Navigate { path } => {
                    lower_expr(env, path, hb.init_navigate())?;
                }
                HandlerAction::Emit { prop, args } => {
                    let Expr::PropsRef { path: prop_path, .. } = prop else {
                        return Err(unsupported(handler.span, "emit of a non-`$props` target"));
                    };
                    let Some(last) = prop_path.last() else {
                        return Err(unsupported(handler.span, "empty emit target"));
                    };
                    let mut emit = hb.init_emit_prop();
                    emit.set_prop(ctx.sym(&last.text));
                    let mut payload = emit.init_payload(args.len() as u32);
                    for (j, arg) in args.iter().enumerate() {
                        lower_expr(env, arg, payload.reborrow().get(j as u32))?;
                    }
                }
            }
        }
    }

    // Children.
    let mut children = w.init_children(widget.children.len() as u32);
    for (i, child) in widget.children.iter().enumerate() {
        path.push(i as u32);
        lower_view(ctx, env, component, path, child, children.reborrow().get(i as u32))?;
        path.pop();
    }
    Ok(())
}

fn lower_modifiers(
    ctx: &Ctx<'_>,
    env: &Env<'_>,
    modifiers: &[ModifierCall],
    widget: &mut ir::widget::Builder<'_>,
) -> Result<(), Diagnostic> {
    // Canonical catalog order (modId ascending).
    let mut sorted: Vec<(u16, &ModifierCall)> = modifiers
        .iter()
        .filter_map(|m| registry::modifier_spec(&m.name.text).map(|(id, _)| (id, m)))
        .collect();
    sorted.sort_by_key(|(id, _)| *id);
    let mut list = widget.reborrow().init_modifiers(sorted.len() as u32);
    for (i, (mod_id, call)) in sorted.iter().enumerate() {
        let mut mb = list.reborrow().get(i as u32);
        mb.set_mod_id(*mod_id);
        let mut args = mb.init_args(call.args.len() as u32);
        for (j, arg) in call.args.iter().enumerate() {
            let mut ab = args.reborrow().get(j as u32);
            match &arg.value {
                Expr::Int { value, .. } => ab.set_int(*value),
                Expr::Fx { value, .. } => ab.set_fx(*value),
                Expr::Bool { value, .. } => ab.set_boolean(*value),
                Expr::Path { segments, .. } if segments.len() == 1 => {
                    ab.set_token(ctx.sym(&segments[0].text));
                }
                other => lower_expr(env, other, ab.init_expr())?,
            }
        }
    }
    Ok(())
}

fn lower_component_ref(
    ctx: &Ctx<'_>,
    env: &mut Env<'_>,
    widget: &WidgetNode,
    builder: ir::view_node::Builder<'_>,
) -> Result<(), Diagnostic> {
    if widget.positional.is_some() {
        return Err(unsupported(widget.span, "positional argument on a component"));
    }
    let mut cr = builder.init_component_ref();
    cr.set_component(
        ctx.component_index.get(widget.name.text.as_str()).copied().unwrap_or(0),
    );
    // Args name-sorted (canonical).
    let mut args: Vec<(&str, &Expr)> =
        widget.props.iter().map(|(name, value)| (name.text.as_str(), value)).collect();
    args.sort_by_key(|(name, _)| *name);
    let mut list = cr.init_args(args.len() as u32);
    for (i, (name, value)) in args.iter().enumerate() {
        let mut ab = list.reborrow().get(i as u32);
        ab.set_name(ctx.sym(name));
        lower_expr(env, value, ab.init_value())?;
    }
    Ok(())
}

// ------------------------------------------------------------------ routes

pub(super) fn build_routes(
    ctx: &Ctx<'_>,
    model: &Model<'_>,
    program: &mut ir::ui_program::Builder<'_>,
) -> Result<(), Diagnostic> {
    // Canonical order: by path.
    let mut order: Vec<usize> = (0..model.routes.len()).collect();
    order.sort_by_key(|&i| model.routes[i].path.as_str());
    let mut routes = program.reborrow().init_routes(model.routes.len() as u32);
    for (i, &model_idx) in order.iter().enumerate() {
        let route = model.routes[model_idx];
        let mut b = routes.reborrow().get(i as u32);
        b.set_path(capnp::text::Reader::from(route.path.as_str()));
        b.set_page(ctx.component_index.get(route.page.text.as_str()).copied().unwrap_or(0));
        let mut params = b.init_params(route.params.len() as u32);
        for (j, (name, ty)) in route.params.iter().enumerate() {
            let mut pb = params.reborrow().get(j as u32);
            pb.set_name(ctx.sym(&name.text));
            lower_type(ty, pb.init_type());
        }
    }
    Ok(())
}
