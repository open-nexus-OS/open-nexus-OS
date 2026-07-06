// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Top-level declarations: Store, Event, reduce, @effect, Component, Page, Routes.

use super::Parser;
use crate::ast::{
    ComponentDecl, EffectDecl, EventCase, EventDecl, Pattern, PageDecl, PropDecl, ReduceArm,
    ReduceDecl, Route, RoutesDecl, StoreDecl, StoreField,
};
use crate::diag::{DiagCode, Diagnostic};
use crate::lexer::TokenKind;
use alloc::{format, string::String, vec::Vec};

impl Parser<'_> {
    /// `Store Name { field: Type = default @persist, ... }`
    pub(super) fn store_decl(&mut self) -> Result<StoreDecl, Diagnostic> {
        let start = self.expect(&TokenKind::KwStore, "`Store`")?;
        let name = self.ident("a store name")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut fields = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            let field_name = self.ident("a field name")?;
            self.expect(&TokenKind::Colon, "`:`")?;
            let ty = self.type_expr()?;
            let default =
                if self.eat(&TokenKind::Eq) { Some(self.expr()?) } else { None };
            let persist = self.eat(&TokenKind::AtPersist);
            let field_span = field_name.span.to(self.prev_span());
            self.expect(&TokenKind::Comma, "`,` after the field")?;
            fields.push(StoreField { name: field_name, ty, default, persist, span: field_span });
        }
        let span = start.to(self.prev_span());
        Ok(StoreDecl { name, fields, span })
    }

    /// `Event Name { Case, Case(Type, ...), ... }`
    pub(super) fn event_decl(&mut self) -> Result<EventDecl, Diagnostic> {
        let start = self.expect(&TokenKind::KwEvent, "`Event`")?;
        let name = self.ident("an event type name")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut cases = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            let case_name = self.ident("an event case name")?;
            let mut payload = Vec::new();
            if self.eat(&TokenKind::LParen) {
                loop {
                    payload.push(self.type_expr()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "`)`")?;
            }
            let case_span = case_name.span.to(self.prev_span());
            self.expect(&TokenKind::Comma, "`,` after the event case")?;
            cases.push(EventCase { name: case_name, payload, span: case_span });
        }
        let span = start.to(self.prev_span());
        Ok(EventDecl { name, cases, span })
    }

    /// `reduce EventName { Case => stmt-or-block, ... }`
    pub(super) fn reduce_decl(&mut self) -> Result<ReduceDecl, Diagnostic> {
        let start = self.expect(&TokenKind::KwReduce, "`reduce`")?;
        let event = self.ident("an event type name")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut arms = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            let pattern = self.pattern()?;
            self.expect(&TokenKind::FatArrow, "`=>`")?;
            let body = self.stmt_or_block()?;
            let arm_span = pattern.span.to(self.prev_span());
            self.expect(&TokenKind::Comma, "`,` after the reducer arm")?;
            arms.push(ReduceArm { pattern, body, span: arm_span });
        }
        if arms.is_empty() {
            return Err(Diagnostic::new(
                DiagCode::EmptyMatch,
                start.to(self.prev_span()),
                String::from("`reduce` needs at least one arm"),
            ));
        }
        let span = start.to(self.prev_span());
        Ok(ReduceDecl { event, arms, span })
    }

    /// `@effect on Case(binds) { stmts }`
    pub(super) fn effect_decl(&mut self) -> Result<EffectDecl, Diagnostic> {
        let start = self.expect(&TokenKind::AtEffect, "`@effect`")?;
        self.expect(&TokenKind::KwOn, "`on`")?;
        let trigger = self.pattern()?;
        let body = self.block()?;
        let span = start.to(self.prev_span());
        Ok(EffectDecl { trigger, body, span })
    }

    /// `CaseName` or `CaseName(bind, ...)`
    pub(super) fn pattern(&mut self) -> Result<Pattern, Diagnostic> {
        let case = self.ident("an event case name")?;
        let mut binds = Vec::new();
        if self.eat(&TokenKind::LParen) {
            loop {
                binds.push(self.ident("a binding name")?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::RParen, "`)`")?;
        }
        let span = case.span.to(self.prev_span());
        Ok(Pattern { case, binds, span })
    }

    /// `Page Name { <view> }`
    pub(super) fn page_decl(&mut self) -> Result<PageDecl, Diagnostic> {
        let start = self.expect(&TokenKind::KwPage, "`Page`")?;
        let name = self.ident("a page name")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let view = self.view_node()?;
        self.expect(&TokenKind::RBrace, "`}` closing the page")?;
        let span = start.to(self.prev_span());
        Ok(PageDecl { name, view, span })
    }

    /// `Component Name { [props: { name: Type, ... }] <view> }`
    pub(super) fn component_decl(&mut self) -> Result<ComponentDecl, Diagnostic> {
        let start = self.expect(&TokenKind::KwComponent, "`Component`")?;
        let name = self.ident("a component name")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut props = Vec::new();
        if self.peek() == &TokenKind::KwProps {
            self.bump();
            self.expect(&TokenKind::Colon, "`:`")?;
            self.expect(&TokenKind::LBrace, "`{`")?;
            while !self.eat(&TokenKind::RBrace) {
                let prop_name = self.ident("a prop name")?;
                self.expect(&TokenKind::Colon, "`:`")?;
                let ty = self.type_expr()?;
                let prop_span = prop_name.span.to(self.prev_span());
                self.expect(&TokenKind::Comma, "`,` after the prop")?;
                props.push(PropDecl { name: prop_name, ty, span: prop_span });
            }
        }
        let mut state = Vec::new();
        if self.peek() == &TokenKind::KwState {
            self.bump();
            self.expect(&TokenKind::Colon, "`:`")?;
            self.expect(&TokenKind::LBrace, "`{`")?;
            while !self.eat(&TokenKind::RBrace) {
                let field_name = self.ident("a state field name")?;
                self.expect(&TokenKind::Colon, "`:`")?;
                let ty = self.type_expr()?;
                let default =
                    if self.eat(&TokenKind::Eq) { Some(self.expr()?) } else { None };
                let field_span = field_name.span.to(self.prev_span());
                self.expect(&TokenKind::Comma, "`,` after the state field")?;
                state.push(crate::ast::StoreField {
                    name: field_name,
                    ty,
                    default,
                    persist: false,
                    span: field_span,
                });
            }
        }
        let view = self.view_node()?;
        self.expect(&TokenKind::RBrace, "`}` closing the component")?;
        let span = start.to(self.prev_span());
        Ok(ComponentDecl { name, props, state, view, span })
    }

    /// `Routes { "/path/:id" -> Page(id: Int); ... }`
    pub(super) fn routes_decl(&mut self) -> Result<RoutesDecl, Diagnostic> {
        let start = self.expect(&TokenKind::KwRoutes, "`Routes`")?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut routes = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            let (path, path_span) = match self.peek() {
                TokenKind::StrLit(path) => {
                    let path = path.clone();
                    let span = self.bump().span;
                    (path, span)
                }
                _ => return Err(self.unexpected("a route path string")),
            };
            if !path.starts_with('/') {
                return Err(Diagnostic::new(
                    DiagCode::InvalidRoutePath,
                    path_span,
                    format!("route path must start with `/`: `{path}`"),
                ));
            }
            self.expect(&TokenKind::Arrow, "`->`")?;
            let page = self.ident("a page name")?;
            let mut params = Vec::new();
            if self.eat(&TokenKind::LParen) {
                loop {
                    let param = self.ident("a parameter name")?;
                    self.expect(&TokenKind::Colon, "`:`")?;
                    let ty = self.type_expr()?;
                    params.push((param, ty));
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "`)`")?;
            }
            let route_span = path_span.to(self.prev_span());
            self.expect(&TokenKind::Semi, "`;` after the route")?;
            routes.push(Route { path, path_span, page, params, span: route_span });
        }
        let span = start.to(self.prev_span());
        Ok(RoutesDecl { routes, span })
    }
}
