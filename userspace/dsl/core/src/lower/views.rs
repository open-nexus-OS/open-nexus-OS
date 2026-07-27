// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Component/view lowering
//! (with persisted NodeIds) and routes.

use super::effects::fill_dispatch;
use super::exprs::{lower_expr, lower_type, Env};
use super::{unsupported, ComponentSource, Ctx};
use crate::ast::{Expr, HandlerAction, ModifierCall, ViewNode, WidgetNode};
use crate::check::Model;
use crate::diag::Diagnostic;
use crate::registry;
use alloc::vec::Vec;
use nexus_dsl_ir::node_id::static_node_id;
use nexus_dsl_ir::ui_ir_capnp as ir;

// -------------------------------------------------------------- components

pub(super) fn build_components(
    ctx: &Ctx<'_>,
    model: &Model<'_>,
    program: &mut ir::ui_program::Builder<'_>,
) -> Result<(), Diagnostic> {
    let mut components = program.reborrow().init_components(ctx.component_order.len() as u32);
    for (i, (name, source)) in ctx.component_order.iter().enumerate() {
        let mut b = components.reborrow().get(i as u32);
        b.set_name(ctx.sym(name));
        let mut env = Env::new(ctx);
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
            // The collection lowers as its WIDGET (the container carrying the
            // authored `.direction/.wrap/.gap/...` modifiers) with ONE ForEach
            // child; the runtime splices the items into the container. The
            // former bare-ForEach lowering DROPPED `collection.modifiers` —
            // every `List(...).direction(row)` silently laid out as a column.
            let mut w = b.init_widget();
            w.set_kind(ctx.sym(&collection.kind.text));
            w.reborrow().init_props(0);
            lower_modifiers(ctx, env, &collection.modifiers, &mut w)?;
            let children = w.init_children(1);
            let mut fe = children.get(0).init_for_each();
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
        // RFC-0084 Phase 2 wires slots into IR v1.5; until then the frontend
        // parses and checks them but lowering refuses — explicitly, so no
        // program silently loses its content regions.
        ViewNode::Slot { span, .. } => {
            Err(unsupported(*span, "`Slot` placeholders (RFC-0084 lands them in IR v1.5)"))
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
                path.push(((i as u32) << 8) | j as u32);
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

    // Auto-synthesized two-way bindings: interactive kind + $state-bound
    // primary prop ⇒ a bind handler (docs/dev/dsl/ir.md v1.2).
    let mut binds: Vec<(u32, &crate::ast::Expr)> = Vec::new();
    for (name, value) in &widget.props {
        if let Expr::StateRef { .. } = value {
            let trigger = match (widget.name.text.as_str(), name.text.as_str()) {
                ("Toggle" | "Checkbox", "checked") => Some("Tap"),
                ("TextField", "value") | ("TextArea", "value") => Some("Change"),
                _ => None,
            };
            if let Some(trigger) = trigger {
                binds.push((ctx.sym(trigger), value));
            }
        }
    }

    // Handlers.
    {
        let mut handlers = w.reborrow().init_handlers((widget.handlers.len() + binds.len()) as u32);
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
        for (i, (trigger, state_ref)) in binds.iter().enumerate() {
            let mut hb = handlers.reborrow().get((widget.handlers.len() + i) as u32);
            hb.set_trigger(*trigger);
            let Expr::StateRef { path, span } = state_ref else { continue };
            let Some(first) = path.first() else {
                return Err(unsupported(*span, "empty binding path"));
            };
            let store = match ctx.store_of_field(&first.text) {
                Ok(store) => store,
                Err(_) => return Err(unsupported(*span, "an unresolvable bound field")),
            };
            let mut get = hb.init_bind();
            get.set_store(store);
            let mut segs = get.init_path(path.len() as u32);
            for (j, seg) in path.iter().enumerate() {
                segs.set(j as u32, ctx.sym(&seg.text));
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
    if !widget.slot_bodies.is_empty() {
        return Err(unsupported(widget.span, "slot bodies (RFC-0084 lands them in IR v1.5)"));
    }
    let mut cr = builder.init_component_ref();
    cr.set_component(ctx.component_index.get(widget.name.text.as_str()).copied().unwrap_or(0));
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
