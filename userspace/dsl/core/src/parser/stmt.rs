// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Statements: reducer bodies (pure) and effect bodies (svc/dispatch allowed —
//! the *checker* enforces which statement kinds are legal where; the parser
//! accepts the union and reports precise spans).

use super::Parser;
use crate::ast::{AssignOp, Stmt, StmtMatchArm};
use crate::diag::{DiagCode, Diagnostic};
use crate::lexer::TokenKind;
use alloc::{string::String, vec, vec::Vec};

impl Parser<'_> {
    /// A single statement, or a `{ ... }` block, normalized to a list.
    pub(super) fn stmt_or_block(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        if self.peek() == &TokenKind::LBrace {
            self.block()
        } else {
            // Single-statement arm sugar: `Case => state.x = expr` (no `;`).
            Ok(vec![self.stmt(false)?])
        }
    }

    /// `{ stmt* }`
    pub(super) fn block(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        let guard = self.enter()?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut stmts = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            stmts.push(self.stmt(true)?);
        }
        self.leave(guard);
        Ok(stmts)
    }

    /// One statement. `in_block` controls the trailing `;` requirement
    /// (single-statement arms omit it; the comma ends the arm).
    fn stmt(&mut self, in_block: bool) -> Result<Stmt, Diagnostic> {
        match self.peek() {
            TokenKind::KwLet => {
                let start = self.bump().span;
                let name = self.ident("a binding name")?;
                self.expect(&TokenKind::Eq, "`=`")?;
                let value = self.expr()?;
                let span = start.to(self.prev_span());
                if in_block {
                    self.expect(&TokenKind::Semi, "`;`")?;
                }
                Ok(Stmt::Let { name, value, span })
            }
            TokenKind::KwIf => {
                let start = self.bump().span;
                let cond = self.expr()?;
                let then = self.block()?;
                let els = if self.eat(&TokenKind::KwElse) {
                    if self.peek() == &TokenKind::KwIf {
                        vec![self.stmt(true)?]
                    } else {
                        self.block()?
                    }
                } else {
                    Vec::new()
                };
                Ok(Stmt::If { cond, then, els, span: start.to(self.prev_span()) })
            }
            TokenKind::KwMatch => {
                let start = self.bump().span;
                let scrutinee = self.expr()?;
                self.expect(&TokenKind::LBrace, "`{`")?;
                let mut arms = Vec::new();
                while !self.eat(&TokenKind::RBrace) {
                    let pattern = self.pattern()?;
                    self.expect(&TokenKind::FatArrow, "`=>`")?;
                    let body = self.stmt_or_block()?;
                    let arm_span = pattern.span.to(self.prev_span());
                    self.expect(&TokenKind::Comma, "`,` after the match arm")?;
                    arms.push(StmtMatchArm { pattern, body, span: arm_span });
                }
                if arms.is_empty() {
                    return Err(Diagnostic::new(
                        DiagCode::EmptyMatch,
                        start.to(self.prev_span()),
                        String::from("`match` needs at least one arm"),
                    ));
                }
                Ok(Stmt::Match { scrutinee, arms, span: start.to(self.prev_span()) })
            }
            TokenKind::KwDispatch => {
                let start = self.bump().span;
                self.expect(&TokenKind::LParen, "`(`")?;
                let case = self.ident("an event case name")?;
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
                self.expect(&TokenKind::RParen, "`)` closing dispatch")?;
                let span = start.to(self.prev_span());
                if in_block {
                    self.expect(&TokenKind::Semi, "`;`")?;
                }
                Ok(Stmt::Dispatch { case, args, span })
            }
            TokenKind::KwState => {
                // `state.field.path <op> expr`
                let start = self.bump().span;
                let mut path = Vec::new();
                while self.eat(&TokenKind::Dot) {
                    path.push(self.ident("a field name")?);
                }
                if path.is_empty() {
                    return Err(self.unexpected("`.field` after `state`"));
                }
                let op = match self.peek() {
                    TokenKind::Eq => AssignOp::Assign,
                    TokenKind::PlusEq => AssignOp::AddAssign,
                    TokenKind::MinusEq => AssignOp::SubAssign,
                    _ => return Err(self.unexpected("`=`, `+=`, or `-=`")),
                };
                self.bump();
                let value = self.expr()?;
                let span = start.to(self.prev_span());
                if in_block {
                    self.expect(&TokenKind::Semi, "`;`")?;
                }
                Ok(Stmt::Assign { path, op, value, span })
            }
            TokenKind::KwSvc => {
                // Bare service-call statement (effects only; checker enforces).
                let expr = self.expr()?;
                let span = expr.span();
                if in_block {
                    self.expect(&TokenKind::Semi, "`;`")?;
                }
                Ok(Stmt::ExprStmt { expr, span })
            }
            _ => Err(self.unexpected(
                "a statement (`let`, `if`, `match`, `dispatch`, `state.…`, or `svc.…`)",
            )),
        }
    }
}
