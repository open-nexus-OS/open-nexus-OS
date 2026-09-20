// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pre-lowering walks over the checked AST.
//!
//! [`collect_symbols`] interns every name the program mentions (plus the i18n
//! keys) BEFORE anything is built — the symbol table is sorted and canonical,
//! so a name that lowering references but this walk missed would emit a
//! dangling symbol id.

use crate::ast::{Decl, File, Stmt, TypeExpr};
use alloc::{collections::BTreeSet, string::String};

pub(super) fn collect_symbols(
    file: &File,
    set: &mut BTreeSet<String>,
    i18n: &mut BTreeSet<String>,
) {
    use crate::ast::{Expr, HandlerAction, ViewNode};

    fn walk_expr(expr: &Expr, set: &mut BTreeSet<String>, i18n: &mut BTreeSet<String>) {
        match expr {
            Expr::EnumLit { ty, case, args, .. } => {
                set.insert(ty.text.clone());
                set.insert(case.text.clone());
                for arg in args {
                    walk_expr(arg, set, i18n);
                }
            }
            Expr::StateRef { path, .. }
            | Expr::PropsRef { path, .. }
            | Expr::DeviceRef { path, .. }
            | Expr::Path { segments: path, .. } => {
                for seg in path {
                    set.insert(seg.text.clone());
                }
            }
            Expr::Call { path, args, .. } => {
                for seg in path {
                    set.insert(seg.text.clone());
                }
                for arg in args {
                    walk_expr(&arg.value, set, i18n);
                }
            }
            Expr::I18n { key, args, .. } => {
                i18n.insert(key.clone());
                set.insert(key.clone());
                for arg in args {
                    walk_expr(arg, set, i18n);
                }
            }
            Expr::List { items, .. } => {
                for item in items {
                    walk_expr(item, set, i18n);
                }
            }
            Expr::Unary { operand, .. } => walk_expr(operand, set, i18n),
            Expr::Binary { lhs, rhs, .. } => {
                walk_expr(lhs, set, i18n);
                walk_expr(rhs, set, i18n);
            }
            _ => {}
        }
    }

    fn walk_stmts(stmts: &[Stmt], set: &mut BTreeSet<String>, i18n: &mut BTreeSet<String>) {
        for stmt in stmts {
            match stmt {
                Stmt::Assign { path, value, .. } => {
                    for seg in path {
                        set.insert(seg.text.clone());
                    }
                    walk_expr(value, set, i18n);
                }
                Stmt::Let { name, value, .. } => {
                    set.insert(name.text.clone());
                    walk_expr(value, set, i18n);
                }
                Stmt::If { cond, then, els, .. } => {
                    walk_expr(cond, set, i18n);
                    walk_stmts(then, set, i18n);
                    walk_stmts(els, set, i18n);
                }
                Stmt::Match { scrutinee, arms, .. } => {
                    walk_expr(scrutinee, set, i18n);
                    for arm in arms {
                        set.insert(arm.pattern.case.text.clone());
                        for bind in &arm.pattern.binds {
                            set.insert(bind.text.clone());
                        }
                        walk_stmts(&arm.body, set, i18n);
                    }
                }
                Stmt::Dispatch { case, args, .. } => {
                    set.insert(case.text.clone());
                    for arg in args {
                        walk_expr(arg, set, i18n);
                    }
                }
                Stmt::ExprStmt { expr, .. } => walk_expr(expr, set, i18n),
            }
        }
    }

    fn walk_type(ty: &TypeExpr, set: &mut BTreeSet<String>) {
        set.insert(ty.name.text.clone());
        for arg in &ty.args {
            walk_type(arg, set);
        }
    }

    fn walk_view(node: &ViewNode, set: &mut BTreeSet<String>, i18n: &mut BTreeSet<String>) {
        match node {
            ViewNode::Widget(widget) => {
                set.insert(widget.name.text.clone());
                // Auto-bind triggers: a $state-bound primary prop on an
                // interactive kind synthesizes a bind handler at lowering —
                // its trigger symbol must exist, or `Ctx::sym` falls back to 0
                // and the handler answers to a trigger nothing fires.
                // Asked of the catalog, exactly as the lowering asks it.
                for (name, value) in &widget.props {
                    if !matches!(value, Expr::StateRef { .. }) {
                        continue;
                    }
                    if let Some(rule) = crate::registry::bind_rule(&widget.name.text, &name.text) {
                        set.insert(alloc::string::String::from(rule.trigger));
                    }
                }
                if let Some(positional) = &widget.positional {
                    walk_expr(positional, set, i18n);
                    // The positional sugar becomes the registry primary prop
                    // during lowering — intern its name here or the emitted
                    // PropInit references a dangling symbol.
                    if let Some(primary) = crate::registry::widget_spec(&widget.name.text)
                        .and_then(|spec| spec.primary_prop)
                    {
                        set.insert(alloc::string::String::from(primary));
                    }
                }
                for (name, value) in &widget.props {
                    set.insert(name.text.clone());
                    walk_expr(value, set, i18n);
                }
                for modifier in &widget.modifiers {
                    for arg in &modifier.args {
                        walk_expr(&arg.value, set, i18n);
                    }
                }
                for handler in &widget.handlers {
                    set.insert(handler.trigger.text.clone());
                    match &handler.action {
                        HandlerAction::Dispatch { case, args } => {
                            set.insert(case.text.clone());
                            for arg in args {
                                walk_expr(arg, set, i18n);
                            }
                        }
                        HandlerAction::Emit { prop, args } => {
                            walk_expr(prop, set, i18n);
                            for arg in args {
                                walk_expr(arg, set, i18n);
                            }
                        }
                        HandlerAction::Navigate { path } => walk_expr(path, set, i18n),
                    }
                }
                for child in &widget.children {
                    walk_view(child, set, i18n);
                }
                // Slot bodies live INSIDE the callsite node — their names
                // must be interned too, or the lowered body would reference
                // dangling symbol ids.
                for binding in &widget.slot_bodies {
                    set.insert(binding.name.text.clone());
                    for child in &binding.body {
                        walk_view(child, set, i18n);
                    }
                }
            }
            ViewNode::If { arms, els, .. } => {
                for (cond, body) in arms {
                    walk_expr(cond, set, i18n);
                    for child in body {
                        walk_view(child, set, i18n);
                    }
                }
                for child in els {
                    walk_view(child, set, i18n);
                }
            }
            ViewNode::For { var, iter, body, .. } => {
                set.insert(var.text.clone());
                walk_expr(iter, set, i18n);
                for child in body {
                    walk_view(child, set, i18n);
                }
            }
            ViewNode::Collection(collection) => {
                set.insert(collection.kind.text.clone());
                set.insert(collection.var.text.clone());
                walk_expr(&collection.binding, set, i18n);
                for modifier in &collection.modifiers {
                    for arg in &modifier.args {
                        walk_expr(&arg.value, set, i18n);
                    }
                }
                for child in &collection.body {
                    walk_view(child, set, i18n);
                }
            }
            ViewNode::Match { scrutinee, arms, .. } => {
                walk_expr(scrutinee, set, i18n);
                for arm in arms {
                    set.insert(arm.pattern.case.text.clone());
                    for child in &arm.body {
                        walk_view(child, set, i18n);
                    }
                }
            }
            ViewNode::Slot { name, .. } => {
                set.insert(name.text.clone());
            }
        }
    }

    for decl in &file.decls {
        match decl {
            Decl::Store(store) => {
                set.insert(store.name.text.clone());
                for field in &store.fields {
                    set.insert(field.name.text.clone());
                    walk_type(&field.ty, set);
                    if let Some(default) = &field.default {
                        walk_expr(default, set, i18n);
                    }
                }
            }
            Decl::Event(event) => {
                set.insert(event.name.text.clone());
                for case in &event.cases {
                    set.insert(case.name.text.clone());
                    for ty in &case.payload {
                        walk_type(ty, set);
                    }
                }
            }
            Decl::Reduce(reduce) => {
                set.insert(reduce.event.text.clone());
                for arm in &reduce.arms {
                    set.insert(arm.pattern.case.text.clone());
                    for bind in &arm.pattern.binds {
                        set.insert(bind.text.clone());
                    }
                    walk_stmts(&arm.body, set, i18n);
                }
            }
            Decl::Effect(effect) => {
                set.insert(effect.trigger.case.text.clone());
                for bind in &effect.trigger.binds {
                    set.insert(bind.text.clone());
                }
                walk_stmts(&effect.body, set, i18n);
            }
            Decl::Page(page) => {
                set.insert(page.name.text.clone());
                walk_view(&page.view, set, i18n);
            }
            Decl::Component(component) => {
                set.insert(component.name.text.clone());
                for prop in &component.props {
                    set.insert(prop.name.text.clone());
                    walk_type(&prop.ty, set);
                }
                for slot in &component.slots {
                    set.insert(slot.text.clone());
                }
                for field in &component.state {
                    set.insert(field.name.text.clone());
                    walk_type(&field.ty, set);
                    if let Some(default) = &field.default {
                        walk_expr(default, set, i18n);
                    }
                    // Implicit store name symbol.
                    set.insert(alloc::format!("__local_{}", component.name.text));
                }
                walk_view(&component.view, set, i18n);
            }
            Decl::Routes(routes) => {
                for route in &routes.routes {
                    set.insert(route.page.text.clone());
                    for (name, ty) in &route.params {
                        set.insert(name.text.clone());
                        walk_type(ty, set);
                    }
                }
            }
            Decl::Query(query) => {
                set.insert(query.name.text.clone());
                set.insert(query.source.text.clone());
                set.insert(query.order_col.text.clone());
                for param in &query.params {
                    set.insert(param.name.text.clone());
                    walk_type(&param.ty, set);
                }
                for pred in &query.preds {
                    set.insert(pred.col.text.clone());
                    walk_expr(&pred.value, set, i18n);
                }
            }
            // Window intent carries no symbols or i18n keys (enum fields only).
            Decl::Window(_) => {}
        }
    }
}
