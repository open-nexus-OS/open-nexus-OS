// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Canonical formatter: AST → the single canonical source layout.
//!
//! `parse → fmt → parse` is idempotent (`fmt(x) == fmt(parse(fmt(x)))`) — a CI
//! invariant. Layout rules are deterministic and width-independent:
//! - 4-space indent; declarations separated by one blank line;
//! - fields/cases/arms/props one per line with trailing `,`;
//! - nodes **with** a `{}` block print modifiers/handlers on their own lines
//!   at node indent; blockless nodes chain modifiers inline;
//! - parentheses are re-emitted exactly where precedence requires them.

use crate::ast::{
    AssignOp, BinOp, CallArg, Decl, Expr, File, HandlerAction, HandlerDecl, ModifierCall,
    Pattern, Stmt, TypeExpr, UnOp, ViewNode, WidgetNode,
};
use alloc::string::String;

const INDENT: &str = "    ";

/// Formats a parsed file into canonical source text.
#[must_use]
pub fn format_file(file: &File) -> String {
    let mut out = String::new();
    for import in &file.imports {
        out.push_str("import \"");
        push_escaped(&mut out, &import.path);
        out.push_str("\"\n");
    }
    if !file.imports.is_empty() && !file.decls.is_empty() {
        out.push('\n');
    }
    for (i, decl) in file.decls.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        decl_to(&mut out, decl);
    }
    out
}

fn indent_to(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str(INDENT);
    }
}

fn push_escaped(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
}

fn decl_to(out: &mut String, decl: &Decl) {
    match decl {
        Decl::Store(store) => {
            out.push_str("Store ");
            out.push_str(&store.name.text);
            out.push_str(" {\n");
            for field in &store.fields {
                indent_to(out, 1);
                out.push_str(&field.name.text);
                out.push_str(": ");
                type_to(out, &field.ty);
                if let Some(default) = &field.default {
                    out.push_str(" = ");
                    expr_to(out, default, 0);
                }
                if field.persist {
                    out.push_str(" @persist");
                }
                out.push_str(",\n");
            }
            out.push_str("}\n");
        }
        Decl::Event(event) => {
            out.push_str("Event ");
            out.push_str(&event.name.text);
            out.push_str(" {\n");
            for case in &event.cases {
                indent_to(out, 1);
                out.push_str(&case.name.text);
                if !case.payload.is_empty() {
                    out.push('(');
                    for (i, ty) in case.payload.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        type_to(out, ty);
                    }
                    out.push(')');
                }
                out.push_str(",\n");
            }
            out.push_str("}\n");
        }
        Decl::Reduce(reduce) => {
            out.push_str("reduce ");
            out.push_str(&reduce.event.text);
            out.push_str(" {\n");
            for arm in &reduce.arms {
                indent_to(out, 1);
                pattern_to(out, &arm.pattern);
                out.push_str(" => ");
                arm_body_to(out, &arm.body, 1);
                out.push_str(",\n");
            }
            out.push_str("}\n");
        }
        Decl::Effect(effect) => {
            out.push_str("@effect on ");
            pattern_to(out, &effect.trigger);
            out.push_str(" {\n");
            for stmt in &effect.body {
                stmt_to(out, stmt, 1);
            }
            out.push_str("}\n");
        }
        Decl::Page(page) => {
            out.push_str("Page ");
            out.push_str(&page.name.text);
            out.push_str(" {\n");
            view_to(out, &page.view, 1);
            out.push_str("}\n");
        }
        Decl::Component(component) => {
            out.push_str("Component ");
            out.push_str(&component.name.text);
            out.push_str(" {\n");
            if !component.props.is_empty() {
                indent_to(out, 1);
                out.push_str("props: {\n");
                for prop in &component.props {
                    indent_to(out, 2);
                    out.push_str(&prop.name.text);
                    out.push_str(": ");
                    type_to(out, &prop.ty);
                    out.push_str(",\n");
                }
                indent_to(out, 1);
                out.push_str("}\n");
            }
            if !component.state.is_empty() {
                indent_to(out, 1);
                out.push_str("state: {\n");
                for field in &component.state {
                    indent_to(out, 2);
                    out.push_str(&field.name.text);
                    out.push_str(": ");
                    type_to(out, &field.ty);
                    if let Some(default) = &field.default {
                        out.push_str(" = ");
                        expr_to(out, default, 0);
                    }
                    out.push_str(",\n");
                }
                indent_to(out, 1);
                out.push_str("}\n");
            }
            view_to(out, &component.view, 1);
            out.push_str("}\n");
        }
        Decl::Query(query) => {
            out.push_str("Query ");
            out.push_str(&query.name.text);
            out.push_str(" on ");
            out.push_str(&query.source.text);
            out.push_str(" {\n");
            // Canonical clause order: params, where (source order), orderBy, limit.
            if !query.params.is_empty() {
                indent_to(out, 1);
                out.push_str("params: {\n");
                for param in &query.params {
                    indent_to(out, 2);
                    out.push_str(&param.name.text);
                    out.push_str(": ");
                    type_to(out, &param.ty);
                    out.push_str(",\n");
                }
                indent_to(out, 1);
                out.push_str("},\n");
            }
            for pred in &query.preds {
                indent_to(out, 1);
                out.push_str("where ");
                out.push_str(&pred.col.text);
                out.push_str(match pred.op {
                    crate::ast::BinOp::Eq => " == ",
                    crate::ast::BinOp::Ge => " >= ",
                    crate::ast::BinOp::Le => " <= ",
                    crate::ast::BinOp::Gt => " > ",
                    _ => " < ",
                });
                expr_to(out, &pred.value, 1);
                out.push_str(",\n");
            }
            indent_to(out, 1);
            out.push_str("orderBy ");
            out.push_str(&query.order_col.text);
            if query.descending {
                out.push_str(" desc");
            }
            out.push_str(",\n");
            indent_to(out, 1);
            out.push_str("limit ");
            out.push_str(&alloc::format!("{}", query.limit));
            out.push_str(",\n");
            out.push_str("}\n");
        }
        Decl::Routes(routes) => {
            out.push_str("Routes {\n");
            for route in &routes.routes {
                indent_to(out, 1);
                out.push('"');
                push_escaped(out, &route.path);
                out.push_str("\" -> ");
                out.push_str(&route.page.text);
                if !route.params.is_empty() {
                    out.push('(');
                    for (i, (name, ty)) in route.params.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        out.push_str(&name.text);
                        out.push_str(": ");
                        type_to(out, ty);
                    }
                    out.push(')');
                }
                out.push_str(";\n");
            }
            out.push_str("}\n");
        }
        Decl::Window(window) => {
            use crate::ast::{WindowLevel, WindowMode, WindowStyle};
            out.push_str("Window {\n");
            indent_to(out, 1);
            out.push_str("style: ");
            out.push_str(match window.style {
                WindowStyle::Titlebar => "titlebar",
                WindowStyle::HiddenTitlebar => "hiddenTitlebar",
                WindowStyle::Plain => "plain",
            });
            out.push_str(",\n");
            indent_to(out, 1);
            out.push_str("mode: ");
            out.push_str(match window.mode {
                WindowMode::Auto => "auto",
                WindowMode::Freeform => "freeform",
                WindowMode::Fullscreen => "fullscreen",
            });
            out.push_str(",\n");
            indent_to(out, 1);
            out.push_str("level: ");
            out.push_str(match window.level {
                WindowLevel::Normal => "normal",
                WindowLevel::Desktop => "desktop",
                WindowLevel::Overlay => "overlay",
            });
            out.push_str(",\n");
            indent_to(out, 1);
            out.push_str("resizable: ");
            out.push_str(if window.resizable { "true" } else { "false" });
            out.push_str(",\n}\n");
        }
    }
}

fn type_to(out: &mut String, ty: &TypeExpr) {
    out.push_str(&ty.name.text);
    if !ty.args.is_empty() {
        out.push('<');
        for (i, arg) in ty.args.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            type_to(out, arg);
        }
        out.push('>');
    }
}

fn pattern_to(out: &mut String, pattern: &Pattern) {
    out.push_str(&pattern.case.text);
    if !pattern.binds.is_empty() {
        out.push('(');
        for (i, bind) in pattern.binds.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&bind.text);
        }
        out.push(')');
    }
}

/// Reducer/match arm body: single simple statement inline, else a block.
fn arm_body_to(out: &mut String, body: &[Stmt], level: usize) {
    let inline = body.len() == 1
        && matches!(
            body[0],
            Stmt::Assign { .. } | Stmt::Let { .. } | Stmt::Dispatch { .. } | Stmt::ExprStmt { .. }
        );
    if inline {
        stmt_inline_to(out, &body[0]);
    } else {
        out.push_str("{\n");
        for stmt in body {
            stmt_to(out, stmt, level + 1);
        }
        indent_to(out, level);
        out.push('}');
    }
}

/// Statement without indentation/terminator (for inline arm bodies).
fn stmt_inline_to(out: &mut String, stmt: &Stmt) {
    match stmt {
        Stmt::Assign { path, op, value, .. } => {
            out.push_str("state");
            for seg in path {
                out.push('.');
                out.push_str(&seg.text);
            }
            out.push_str(match op {
                AssignOp::Assign => " = ",
                AssignOp::AddAssign => " += ",
                AssignOp::SubAssign => " -= ",
            });
            expr_to(out, value, 0);
        }
        Stmt::Let { name, value, .. } => {
            out.push_str("let ");
            out.push_str(&name.text);
            out.push_str(" = ");
            expr_to(out, value, 0);
        }
        Stmt::Dispatch { case, args, .. } => {
            out.push_str("dispatch(");
            out.push_str(&case.text);
            if !args.is_empty() {
                out.push('(');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    expr_to(out, arg, 0);
                }
                out.push(')');
            }
            out.push(')');
        }
        Stmt::ExprStmt { expr, .. } => expr_to(out, expr, 0),
        Stmt::If { .. } | Stmt::Match { .. } => {
            // Not inline-able; arm_body_to never routes these here.
        }
    }
}

fn stmt_to(out: &mut String, stmt: &Stmt, level: usize) {
    match stmt {
        Stmt::Assign { .. } | Stmt::Let { .. } | Stmt::Dispatch { .. } | Stmt::ExprStmt { .. } => {
            indent_to(out, level);
            stmt_inline_to(out, stmt);
            out.push_str(";\n");
        }
        Stmt::If { cond, then, els, .. } => {
            indent_to(out, level);
            out.push_str("if ");
            expr_to(out, cond, 0);
            out.push_str(" {\n");
            for s in then {
                stmt_to(out, s, level + 1);
            }
            indent_to(out, level);
            out.push('}');
            if !els.is_empty() {
                out.push_str(" else {\n");
                for s in els {
                    stmt_to(out, s, level + 1);
                }
                indent_to(out, level);
                out.push('}');
            }
            out.push('\n');
        }
        Stmt::Match { scrutinee, arms, .. } => {
            indent_to(out, level);
            out.push_str("match ");
            expr_to(out, scrutinee, 0);
            out.push_str(" {\n");
            for arm in arms {
                indent_to(out, level + 1);
                pattern_to(out, &arm.pattern);
                out.push_str(" => ");
                arm_body_to(out, &arm.body, level + 1);
                out.push_str(",\n");
            }
            indent_to(out, level);
            out.push_str("}\n");
        }
    }
}

// ---------------------------------------------------------------- views

fn view_to(out: &mut String, node: &ViewNode, level: usize) {
    match node {
        ViewNode::Widget(widget) => widget_to(out, widget, level),
        ViewNode::If { arms, els, .. } => {
            indent_to(out, level);
            for (i, (cond, body)) in arms.iter().enumerate() {
                if i > 0 {
                    out.push_str(" else if ");
                } else {
                    out.push_str("if ");
                }
                expr_to(out, cond, 0);
                out.push_str(" {\n");
                for child in body {
                    view_to(out, child, level + 1);
                }
                indent_to(out, level);
                out.push('}');
            }
            if !els.is_empty() {
                out.push_str(" else {\n");
                for child in els {
                    view_to(out, child, level + 1);
                }
                indent_to(out, level);
                out.push('}');
            }
            out.push('\n');
        }
        ViewNode::For { var, iter, body, .. } => {
            indent_to(out, level);
            out.push_str("for ");
            out.push_str(&var.text);
            out.push_str(" in ");
            expr_to(out, iter, 0);
            out.push_str(" {\n");
            for child in body {
                view_to(out, child, level + 1);
            }
            indent_to(out, level);
            out.push_str("}\n");
        }
        ViewNode::Match { scrutinee, arms, .. } => {
            indent_to(out, level);
            out.push_str("match ");
            expr_to(out, scrutinee, 0);
            out.push_str(" {\n");
            for arm in arms {
                indent_to(out, level + 1);
                pattern_to(out, &arm.pattern);
                out.push_str(" => {\n");
                for child in &arm.body {
                    view_to(out, child, level + 2);
                }
                indent_to(out, level + 1);
                out.push_str("},\n");
            }
            indent_to(out, level);
            out.push_str("}\n");
        }
        ViewNode::Collection(collection) => {
            indent_to(out, level);
            out.push_str(&collection.kind.text);
            out.push('(');
            expr_to(out, &collection.binding, 0);
            out.push_str(") { ");
            out.push_str(&collection.var.text);
            out.push_str(" in\n");
            for child in &collection.body {
                view_to(out, child, level + 1);
            }
            indent_to(out, level);
            out.push('}');
            block_modifiers_to(out, &collection.modifiers, level);
            out.push('\n');
        }
    }
}

fn widget_to(out: &mut String, widget: &WidgetNode, level: usize) {
    indent_to(out, level);
    out.push_str(&widget.name.text);
    if let Some(positional) = &widget.positional {
        out.push('(');
        expr_to(out, positional, 0);
        out.push(')');
    }
    let has_block = !widget.props.is_empty() || !widget.children.is_empty();
    if has_block {
        out.push_str(" {\n");
        for (name, value) in &widget.props {
            indent_to(out, level + 1);
            out.push_str(&name.text);
            out.push_str(": ");
            expr_to(out, value, 0);
            out.push_str(",\n");
        }
        for child in &widget.children {
            view_to(out, child, level + 1);
        }
        indent_to(out, level);
        out.push('}');
        block_modifiers_to(out, &widget.modifiers, level);
    } else {
        // Blockless node: modifiers chain inline.
        for modifier in &widget.modifiers {
            modifier_to(out, modifier);
        }
    }
    for handler in &widget.handlers {
        out.push('\n');
        indent_to(out, level);
        handler_to(out, handler);
    }
    out.push('\n');
}

/// Modifiers of a block node: each on its own line at node indent.
fn block_modifiers_to(out: &mut String, modifiers: &[ModifierCall], level: usize) {
    for modifier in modifiers {
        out.push('\n');
        indent_to(out, level);
        modifier_to(out, modifier);
    }
}

fn modifier_to(out: &mut String, modifier: &ModifierCall) {
    out.push('.');
    out.push_str(&modifier.name.text);
    out.push('(');
    call_args_to(out, &modifier.args);
    out.push(')');
}

fn handler_to(out: &mut String, handler: &HandlerDecl) {
    out.push_str("on ");
    out.push_str(&handler.trigger.text);
    out.push_str(" -> ");
    match &handler.action {
        HandlerAction::Dispatch { case, args } => {
            out.push_str("dispatch(");
            out.push_str(&case.text);
            if !args.is_empty() {
                out.push('(');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    expr_to(out, arg, 0);
                }
                out.push(')');
            }
            out.push(')');
        }
        HandlerAction::Navigate { path } => {
            out.push_str("navigate(");
            expr_to(out, path, 0);
            out.push(')');
        }
        HandlerAction::Emit { prop, args } => {
            out.push_str("emit(");
            expr_to(out, prop, 0);
            for arg in args {
                out.push_str(", ");
                expr_to(out, arg, 0);
            }
            out.push(')');
        }
    }
}

// ---------------------------------------------------------- expressions

/// Binding strength; parent passes its level, children parenthesize if weaker.
fn precedence(expr: &Expr) -> u8 {
    match expr {
        Expr::Binary { op, .. } => match op {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 3,
            BinOp::Add | BinOp::Sub => 4,
            BinOp::Mul | BinOp::Div | BinOp::Rem => 5,
        },
        Expr::Unary { .. } => 6,
        _ => 7,
    }
}

fn expr_to(out: &mut String, expr: &Expr, min_prec: u8) {
    let prec = precedence(expr);
    let parens = prec < min_prec;
    if parens {
        out.push('(');
    }
    match expr {
        Expr::Bool { value, .. } => out.push_str(if *value { "true" } else { "false" }),
        Expr::Int { value, .. } => out.push_str(&alloc::format!("{value}")),
        Expr::Fx { value, .. } => fx_to(out, *value),
        Expr::Str { value, .. } => {
            out.push('"');
            push_escaped(out, value);
            out.push('"');
        }
        Expr::List { items, .. } => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                expr_to(out, item, 0);
            }
            out.push(']');
        }
        Expr::EnumLit { ty, case, args, .. } => {
            out.push_str(&ty.text);
            out.push_str("::");
            out.push_str(&case.text);
            if !args.is_empty() {
                out.push('(');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    expr_to(out, arg, 0);
                }
                out.push(')');
            }
        }
        Expr::StateRef { path, .. } => {
            out.push_str("$state");
            for seg in path {
                out.push('.');
                out.push_str(&seg.text);
            }
        }
        Expr::PropsRef { path, .. } => {
            out.push_str("$props");
            for seg in path {
                out.push('.');
                out.push_str(&seg.text);
            }
        }
        Expr::DeviceRef { path, .. } => {
            out.push_str("device");
            for seg in path {
                out.push('.');
                out.push_str(&seg.text);
            }
        }
        Expr::Path { segments, .. } => {
            for (i, seg) in segments.iter().enumerate() {
                if i > 0 {
                    out.push('.');
                }
                out.push_str(&seg.text);
            }
        }
        Expr::Call { path, args, .. } => {
            for (i, seg) in path.iter().enumerate() {
                if i > 0 {
                    out.push('.');
                }
                out.push_str(&seg.text);
            }
            out.push('(');
            call_args_to(out, args);
            out.push(')');
        }
        Expr::I18n { key, args, .. } => {
            out.push_str("@t(\"");
            push_escaped(out, key);
            out.push('"');
            for arg in args {
                out.push_str(", ");
                expr_to(out, arg, 0);
            }
            out.push(')');
        }
        Expr::Unary { op, operand, .. } => {
            out.push(match op {
                UnOp::Not => '!',
                UnOp::Neg => '-',
            });
            expr_to(out, operand, 6);
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            expr_to(out, lhs, prec);
            out.push_str(match op {
                BinOp::Or => " || ",
                BinOp::And => " && ",
                BinOp::Eq => " == ",
                BinOp::Ne => " != ",
                BinOp::Lt => " < ",
                BinOp::Le => " <= ",
                BinOp::Gt => " > ",
                BinOp::Ge => " >= ",
                BinOp::Add => " + ",
                BinOp::Sub => " - ",
                BinOp::Mul => " * ",
                BinOp::Div => " / ",
                BinOp::Rem => " % ",
            });
            // Left-associative: the rhs needs strictly tighter binding.
            expr_to(out, rhs, prec + 1);
        }
    }
    if parens {
        out.push(')');
    }
}

fn call_args_to(out: &mut String, args: &[CallArg]) {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        if let Some(name) = &arg.name {
            out.push_str(&name.text);
            out.push_str(": ");
        }
        expr_to(out, &arg.value, 0);
    }
}

/// Prints a Q32.32 literal as decimal with an exact round-trip guarantee.
///
/// 10 fractional digits uniquely identify a 2^-32 step (10^-10 / 2 < 2^-33),
/// and the lexer's half-up rounding maps the printed value back to the same
/// raw — so `fmt ∘ parse ∘ fmt = fmt` holds for `Fx` literals.
fn fx_to(out: &mut String, raw: i64) {
    let negative = raw < 0;
    let magnitude = raw.unsigned_abs();
    let int_part = magnitude >> 32;
    let frac = magnitude & 0xffff_ffff;
    if negative {
        out.push('-');
    }
    out.push_str(&alloc::format!("{int_part}"));
    out.push('.');
    // Round frac/2^32 to 10 decimal digits, half-up.
    let scaled: u128 = (u128::from(frac) * 10u128.pow(10) + (1u128 << 31)) >> 32;
    let mut digits = alloc::format!("{scaled:010}");
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    out.push_str(&digits);
}
