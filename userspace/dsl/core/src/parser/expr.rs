// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Expressions — precedence-climbing over the bounded operator set.
//! Precedence (loose→tight): `||` < `&&` < comparisons < `+ -` < `* / %` < unary.

use super::Parser;
use crate::ast::{BinOp, CallArg, Expr, Ident, UnOp};
use crate::diag::Diagnostic;
use crate::lexer::TokenKind;
use alloc::{boxed::Box, string::String, vec::Vec};

impl Parser<'_> {
    pub(super) fn expr(&mut self) -> Result<Expr, Diagnostic> {
        let guard = self.enter()?;
        let expr = self.or_expr()?;
        self.leave(guard);
        Ok(expr)
    }

    fn or_expr(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.and_expr()?;
        while self.eat(&TokenKind::OrOr) {
            let rhs = self.and_expr()?;
            lhs = binary(BinOp::Or, lhs, rhs);
        }
        Ok(lhs)
    }

    fn and_expr(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.cmp_expr()?;
        while self.eat(&TokenKind::AndAnd) {
            let rhs = self.cmp_expr()?;
            lhs = binary(BinOp::And, lhs, rhs);
        }
        Ok(lhs)
    }

    fn cmp_expr(&mut self) -> Result<Expr, Diagnostic> {
        let lhs = self.add_expr()?;
        let op = match self.peek() {
            TokenKind::EqEq => BinOp::Eq,
            TokenKind::Ne => BinOp::Ne,
            TokenKind::Lt => BinOp::Lt,
            TokenKind::Le => BinOp::Le,
            TokenKind::Gt => BinOp::Gt,
            TokenKind::Ge => BinOp::Ge,
            _ => return Ok(lhs),
        };
        self.bump();
        let rhs = self.add_expr()?;
        Ok(binary(op, lhs, rhs))
    }

    fn add_expr(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.mul_expr()?;
            lhs = binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn mul_expr(&mut self) -> Result<Expr, Diagnostic> {
        let mut lhs = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem,
                _ => break,
            };
            self.bump();
            let rhs = self.unary_expr()?;
            lhs = binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn unary_expr(&mut self) -> Result<Expr, Diagnostic> {
        let op = match self.peek() {
            TokenKind::Bang => Some(UnOp::Not),
            TokenKind::Minus => Some(UnOp::Neg),
            _ => None,
        };
        if let Some(op) = op {
            let start = self.bump().span;
            let operand = self.unary_expr()?;
            let span = start.to(operand.span());
            return Ok(Expr::Unary { op, operand: Box::new(operand), span });
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let guard = self.enter()?;
        let expr = self.primary_inner()?;
        self.leave(guard);
        Ok(expr)
    }

    fn primary_inner(&mut self) -> Result<Expr, Diagnostic> {
        match self.peek().clone() {
            TokenKind::KwTrue => Ok(Expr::Bool { value: true, span: self.bump().span }),
            TokenKind::KwFalse => Ok(Expr::Bool { value: false, span: self.bump().span }),
            TokenKind::IntLit(value) => Ok(Expr::Int { value, span: self.bump().span }),
            TokenKind::FxLit(value) => Ok(Expr::Fx { value, span: self.bump().span }),
            TokenKind::StrLit(value) => Ok(Expr::Str { value, span: self.bump().span }),
            TokenKind::LParen => {
                self.bump();
                let inner = self.expr()?;
                self.expect(&TokenKind::RParen, "`)`")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                let start = self.bump().span;
                let mut items = Vec::new();
                if self.peek() != &TokenKind::RBracket {
                    loop {
                        items.push(self.expr()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let end = self.expect(&TokenKind::RBracket, "`]`")?;
                Ok(Expr::List { items, span: start.to(end) })
            }
            TokenKind::AtT => {
                let start = self.bump().span;
                self.expect(&TokenKind::LParen, "`(`")?;
                let (key, key_span) = match self.peek() {
                    TokenKind::StrLit(key) => {
                        let key = key.clone();
                        let span = self.bump().span;
                        (key, span)
                    }
                    _ => return Err(self.unexpected("a translation key string")),
                };
                let mut args = Vec::new();
                while self.eat(&TokenKind::Comma) {
                    args.push(self.expr()?);
                }
                let end = self.expect(&TokenKind::RParen, "`)`")?;
                Ok(Expr::I18n { key, key_span, args, span: start.to(end) })
            }
            TokenKind::DollarState => {
                let start = self.bump().span;
                let path = self.dot_path("a state field")?;
                Ok(Expr::StateRef { path, span: start.to(self.prev_span()) })
            }
            TokenKind::DollarProps => {
                let start = self.bump().span;
                let path = self.dot_path("a prop name")?;
                Ok(Expr::PropsRef { path, span: start.to(self.prev_span()) })
            }
            TokenKind::KwDevice => {
                let start = self.bump().span;
                let path = self.dot_path("a device field")?;
                Ok(Expr::DeviceRef { path, span: start.to(self.prev_span()) })
            }
            TokenKind::KwState => {
                // Reducer-body state reads: `state.field`.
                let start = self.bump().span;
                let path = self.dot_path("a state field")?;
                Ok(Expr::StateRef { path, span: start.to(self.prev_span()) })
            }
            TokenKind::KwSvc => {
                // `svc.service.method(args)`
                let start = self.bump().span;
                let mut path = Vec::new();
                path.push(Ident { text: String::from("svc"), span: start });
                let mut rest = self.dot_path("a service path")?;
                path.append(&mut rest);
                self.expect(&TokenKind::LParen, "`(` (service calls need arguments)")?;
                let args = self.call_args()?;
                Ok(Expr::Call { path, args, span: start.to(self.prev_span()) })
            }
            TokenKind::Ident(_) => {
                let first = self.ident("a name")?;
                // `Type::Case` / `Type::Case(args)`
                if self.eat(&TokenKind::ColonColon) {
                    let case = self.ident("an enum case")?;
                    let mut args = Vec::new();
                    if self.eat(&TokenKind::LParen) {
                        if self.peek() != &TokenKind::RParen {
                            loop {
                                args.push(self.expr()?);
                                if !self.eat(&TokenKind::Comma) {
                                    break;
                                }
                            }
                        }
                        self.expect(&TokenKind::RParen, "`)`")?;
                    }
                    let span = first.span.to(self.prev_span());
                    return Ok(Expr::EnumLit { ty: first, case, args, span });
                }
                // Path (`user.name`) with optional trailing call (`f(args)`).
                let mut segments = Vec::new();
                segments.push(first);
                while self.peek() == &TokenKind::Dot
                    && matches!(self.peek_at(1), TokenKind::Ident(_))
                {
                    self.bump();
                    segments.push(self.ident("a field name")?);
                }
                if self.eat(&TokenKind::LParen) {
                    let args = self.call_args()?;
                    let span = segments[0].span.to(self.prev_span());
                    return Ok(Expr::Call { path: segments, args, span });
                }
                let span = segments[0].span.to(self.prev_span());
                Ok(Expr::Path { segments, span })
            }
            _ => Err(self.unexpected("an expression")),
        }
    }

    /// `.a.b.c` — one or more dot segments.
    fn dot_path(&mut self, what: &str) -> Result<Vec<Ident>, Diagnostic> {
        let mut path = Vec::new();
        self.expect(&TokenKind::Dot, "`.`")?;
        path.push(self.ident(what)?);
        while self.peek() == &TokenKind::Dot && matches!(self.peek_at(1), TokenKind::Ident(_)) {
            self.bump();
            path.push(self.ident(what)?);
        }
        Ok(path)
    }

    /// Call arguments up to `)`: `expr` and `name: expr` mixed.
    pub(super) fn call_args(&mut self) -> Result<Vec<CallArg>, Diagnostic> {
        let mut args = Vec::new();
        if self.peek() != &TokenKind::RParen {
            loop {
                let name = if matches!(self.peek(), TokenKind::Ident(_))
                    && self.peek_at(1) == &TokenKind::Colon
                {
                    let name = self.ident("an argument name")?;
                    self.bump(); // ':'
                    Some(name)
                } else {
                    None
                };
                let value = self.expr()?;
                args.push(CallArg { name, value });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "`)`")?;
        Ok(args)
    }
}

fn binary(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    let span = lhs.span().to(rhs.span());
    Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span }
}
