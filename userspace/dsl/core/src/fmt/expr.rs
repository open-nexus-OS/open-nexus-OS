// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The canonical printer for the leaf grammar: types, patterns and
//! expressions. Parentheses are re-emitted exactly where precedence requires
//! them (`precedence` is the single table both sides of `parse → fmt → parse`
//! agree on), and fixed-point literals round-trip through `fx_to`.

use super::push_escaped;
use crate::ast::{BinOp, CallArg, Expr, Pattern, TypeExpr, UnOp};
use alloc::string::String;

pub(super) fn type_to(out: &mut String, ty: &TypeExpr) {
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

pub(super) fn pattern_to(out: &mut String, pattern: &Pattern) {
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

pub(super) fn expr_to(out: &mut String, expr: &Expr, min_prec: u8) {
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

pub(super) fn call_args_to(out: &mut String, args: &[CallArg]) {
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
