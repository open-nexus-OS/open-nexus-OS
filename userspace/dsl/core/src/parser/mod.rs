// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Recursive-descent parser for the `.nx` v1 surface.
//!
//! Fail-fast: the first violation returns a [`Diagnostic`] with a stable code
//! and span. Nesting is bounded ([`MAX_NESTING`]) — a hard determinism/bounds
//! requirement, not a style choice.

mod decls;
mod expr;
mod stmt;
mod view;

use crate::ast::{Decl, File, Ident, Import, TypeExpr};
use crate::diag::{DiagCode, Diagnostic, Span};
use crate::lexer::{lex, Token, TokenKind};
use alloc::{format, string::String, vec::Vec};

/// Maximum structural nesting depth (blocks/exprs/views combined).
pub const MAX_NESTING: u32 = 64;

/// Parses one source file.
///
/// # Errors
/// The first lexical or syntactic violation.
pub fn parse_file(source: &str) -> Result<File, Diagnostic> {
    let tokens = lex(source)?;
    let mut parser = Parser { tokens: &tokens, pos: 0, depth: 0 };
    let file = parser.file()?;
    Ok(file)
}

pub(crate) struct Parser<'t> {
    tokens: &'t [Token],
    pos: usize,
    depth: u32,
}

impl<'t> Parser<'t> {
    // ------------------------------------------------------------- cursor

    pub(crate) fn peek(&self) -> &'t TokenKind {
        &self.tokens[self.pos.min(self.tokens.len() - 1)].kind
    }

    pub(crate) fn peek_at(&self, offset: usize) -> &'t TokenKind {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx].kind
    }

    pub(crate) fn span(&self) -> Span {
        self.tokens[self.pos.min(self.tokens.len() - 1)].span
    }

    pub(crate) fn prev_span(&self) -> Span {
        self.tokens[self.pos.saturating_sub(1).min(self.tokens.len() - 1)].span
    }

    pub(crate) fn bump(&mut self) -> &'t Token {
        let token = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    pub(crate) fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.peek() == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Span, Diagnostic> {
        if self.peek() == kind {
            Ok(self.bump().span)
        } else {
            Err(self.unexpected(what))
        }
    }

    pub(crate) fn unexpected(&self, what: &str) -> Diagnostic {
        Diagnostic::new(
            DiagCode::UnexpectedToken,
            self.span(),
            format!("expected {what}, found {:?}", self.peek()),
        )
    }

    pub(crate) fn enter(&mut self) -> Result<DepthGuard, Diagnostic> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return Err(Diagnostic::new(
                DiagCode::NestingTooDeep,
                self.span(),
                format!("nesting exceeds {MAX_NESTING} levels"),
            ));
        }
        Ok(DepthGuard)
    }

    pub(crate) fn leave(&mut self, _guard: DepthGuard) {
        self.depth -= 1;
    }

    // ------------------------------------------------------------ shared

    pub(crate) fn ident(&mut self, what: &str) -> Result<Ident, Diagnostic> {
        match self.peek() {
            TokenKind::Ident(text) => {
                let text = text.clone();
                let span = self.bump().span;
                Ok(Ident { text, span })
            }
            _ => Err(self.unexpected(what)),
        }
    }

    /// `Name` or `Name<T, U>` (angle-bracket type arguments).
    pub(crate) fn type_expr(&mut self) -> Result<TypeExpr, Diagnostic> {
        let guard = self.enter()?;
        let name = self.ident("a type name")?;
        let mut args = Vec::new();
        let mut span = name.span;
        if self.eat(&TokenKind::Lt) {
            loop {
                args.push(self.type_expr()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            span = span.to(self.expect(&TokenKind::Gt, "`>`")?);
        }
        self.leave(guard);
        Ok(TypeExpr { name, args, span })
    }

    // -------------------------------------------------------------- file

    fn file(&mut self) -> Result<File, Diagnostic> {
        let mut imports = Vec::new();
        while self.peek() == &TokenKind::KwImport {
            let start = self.bump().span;
            match self.peek() {
                TokenKind::StrLit(path) => {
                    let path = path.clone();
                    let end = self.bump().span;
                    imports.push(Import { path, span: start.to(end) });
                }
                _ => return Err(self.unexpected("an import path string")),
            }
        }
        let mut decls = Vec::new();
        while self.peek() != &TokenKind::Eof {
            decls.push(self.decl()?);
        }
        if self.pos < self.tokens.len() - 1 {
            return Err(Diagnostic::new(
                DiagCode::TrailingTokens,
                self.span(),
                String::from("trailing tokens after declarations"),
            ));
        }
        Ok(File { imports, decls })
    }

    fn decl(&mut self) -> Result<Decl, Diagnostic> {
        match self.peek() {
            TokenKind::KwStore => self.store_decl().map(Decl::Store),
            TokenKind::KwEvent => self.event_decl().map(Decl::Event),
            TokenKind::KwReduce => self.reduce_decl().map(Decl::Reduce),
            TokenKind::AtEffect => self.effect_decl().map(Decl::Effect),
            TokenKind::KwComponent => self.component_decl().map(Decl::Component),
            TokenKind::KwPage => self.page_decl().map(Decl::Page),
            TokenKind::KwRoutes => self.routes_decl().map(Decl::Routes),
            _ => Err(self.unexpected(
                "a declaration (Store, Event, reduce, @effect, Component, Page, Routes)",
            )),
        }
    }
}

pub(crate) struct DepthGuard;
