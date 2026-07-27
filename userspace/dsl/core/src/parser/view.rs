// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! View nodes: widgets/components, `if`/`for`/`match`, collection templates,
//! modifiers, handlers.

use super::Parser;
use crate::ast::{
    CollectionNode, Expr, HandlerAction, HandlerDecl, ModifierCall, SlotBinding, ViewMatchArm,
    ViewNode, WidgetNode,
};
use crate::diag::{DiagCode, Diagnostic};
use crate::lexer::TokenKind;
use alloc::{string::String, vec::Vec};

impl Parser<'_> {
    pub(super) fn view_node(&mut self) -> Result<ViewNode, Diagnostic> {
        let guard = self.enter()?;
        let node = self.view_node_inner()?;
        self.leave(guard);
        Ok(node)
    }

    fn view_node_inner(&mut self) -> Result<ViewNode, Diagnostic> {
        match self.peek() {
            TokenKind::KwIf => self.if_view(),
            TokenKind::KwFor => self.for_view(),
            TokenKind::KwMatch => self.match_view(),
            // `Slot <name>` — the component content-region placeholder
            // (RFC-0084). `Slot` is a contextual identifier, not a keyword,
            // so `slot`/`Slot` stay usable as prop and field names; the
            // lookahead on a following identifier is what disambiguates it
            // from a component actually named `Slot` (which the checker
            // forbids anyway).
            TokenKind::Ident(name) if name == "Slot" && self.slot_placeholder_ahead() => {
                self.slot_view()
            }
            TokenKind::Ident(_) => self.widget_like(),
            _ => Err(self.unexpected("a view node (widget, `if`, `for`, `match`)")),
        }
    }

    /// `if cond { .. } else if cond { .. } else { .. }` — flattened arms.
    fn if_view(&mut self) -> Result<ViewNode, Diagnostic> {
        let start = self.expect(&TokenKind::KwIf, "`if`")?;
        let mut arms = Vec::new();
        let mut els = Vec::new();
        loop {
            let cond = self.expr()?;
            let body = self.view_block()?;
            arms.push((cond, body));
            if !self.eat(&TokenKind::KwElse) {
                break;
            }
            if self.eat(&TokenKind::KwIf) {
                continue;
            }
            els = self.view_block()?;
            break;
        }
        Ok(ViewNode::If { arms, els, span: start.to(self.prev_span()) })
    }

    /// `for var in expr { view* }`
    fn for_view(&mut self) -> Result<ViewNode, Diagnostic> {
        let start = self.expect(&TokenKind::KwFor, "`for`")?;
        let var = self.ident("a loop variable")?;
        self.expect(&TokenKind::KwIn, "`in`")?;
        let iter = self.expr()?;
        let body = self.view_block()?;
        Ok(ViewNode::For { var, iter, body, span: start.to(self.prev_span()) })
    }

    /// `match expr { Case => { view* }, }`
    fn match_view(&mut self) -> Result<ViewNode, Diagnostic> {
        let start = self.expect(&TokenKind::KwMatch, "`match`")?;
        let scrutinee = self.expr()?;
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut arms = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            let pattern = self.pattern()?;
            self.expect(&TokenKind::FatArrow, "`=>`")?;
            let body = self.view_block()?;
            let arm_span = pattern.span.to(self.prev_span());
            self.expect(&TokenKind::Comma, "`,` after the match arm")?;
            arms.push(ViewMatchArm { pattern, body, span: arm_span });
        }
        if arms.is_empty() {
            return Err(Diagnostic::new(
                DiagCode::EmptyMatch,
                start.to(self.prev_span()),
                String::from("`match` needs at least one arm"),
            ));
        }
        Ok(ViewNode::Match { scrutinee, arms, span: start.to(self.prev_span()) })
    }

    /// `{ view* }`
    fn view_block(&mut self) -> Result<Vec<ViewNode>, Diagnostic> {
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut nodes = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            nodes.push(self.view_node()?);
        }
        Ok(nodes)
    }

    /// `Slot <name>` reads as a placeholder only when a bare identifier
    /// follows that does NOT open a node of its own — `Slot Text("hi")` and
    /// `Slot Row { }` stay two sibling widgets, and a placeholder never takes
    /// modifiers, so a following `.` also rules it out.
    fn slot_placeholder_ahead(&self) -> bool {
        matches!(self.peek_at(1), TokenKind::Ident(_))
            && !matches!(self.peek_at(2), TokenKind::LParen | TokenKind::LBrace | TokenKind::Dot)
    }

    /// `Slot <name>` (RFC-0084) — where a caller's slot body lands.
    fn slot_view(&mut self) -> Result<ViewNode, Diagnostic> {
        let start = self.bump().span; // `Slot`
        let name = self.ident("a slot name")?;
        Ok(ViewNode::Slot { name, span: start.to(self.prev_span()) })
    }

    /// The callsite slot block: a SECOND brace block after the prop block,
    /// `{ <slot> { view* } … }`. Unambiguous because after a node's prop
    /// block the grammar admits only `.modifier`, `on …`, or the next
    /// sibling — and a sibling never starts with `{`.
    fn slot_block(&mut self) -> Result<Vec<SlotBinding>, Diagnostic> {
        self.expect(&TokenKind::LBrace, "`{` opening the slot block")?;
        let mut bindings = Vec::new();
        while !self.eat(&TokenKind::RBrace) {
            let name = self.ident("a slot name")?;
            let body = self.view_block()?;
            let span = name.span.to(self.prev_span());
            bindings.push(SlotBinding { name, body, span });
            let _ = self.eat(&TokenKind::Semi) || self.eat(&TokenKind::Comma);
        }
        Ok(bindings)
    }

    /// `Name`, `Name(positional)`, `Name { ... }`, `Name(expr) { item in ... }`
    /// — plus trailing modifiers and handlers.
    fn widget_like(&mut self) -> Result<ViewNode, Diagnostic> {
        let name = self.ident("a widget or component name")?;
        let start = name.span;

        let mut positional = None;
        if self.eat(&TokenKind::LParen) {
            positional = Some(self.expr()?);
            self.expect(&TokenKind::RParen, "`)`")?;
        }

        // Collection template: `Name(expr) { item in ... }`
        if positional.is_some()
            && self.peek() == &TokenKind::LBrace
            && matches!(self.peek_at(1), TokenKind::Ident(_))
            && self.peek_at(2) == &TokenKind::KwIn
        {
            self.bump(); // '{'
            let var = self.ident("the item binding")?;
            self.bump(); // 'in'
            let mut body = Vec::new();
            while !self.eat(&TokenKind::RBrace) {
                body.push(self.view_node()?);
            }
            let modifiers = self.modifiers()?;
            let binding = positional.take().unwrap_or(Expr::Bool { value: false, span: start });
            return Ok(ViewNode::Collection(CollectionNode {
                kind: name,
                binding,
                var,
                body,
                modifiers,
                span: start.to(self.prev_span()),
            }));
        }

        let mut props = Vec::new();
        let mut children = Vec::new();
        let mut handlers = Vec::new();
        if self.eat(&TokenKind::LBrace) {
            while !self.eat(&TokenKind::RBrace) {
                if self.peek() == &TokenKind::KwOn {
                    handlers.push(self.handler()?);
                    // Optional separator after inline handlers.
                    let _ = self.eat(&TokenKind::Semi) || self.eat(&TokenKind::Comma);
                    continue;
                }
                let is_prop = matches!(self.peek(), TokenKind::Ident(_))
                    && self.peek_at(1) == &TokenKind::Colon;
                if is_prop {
                    let prop_name = self.ident("a property name")?;
                    if props.iter().any(|(existing, _): &(crate::ast::Ident, Expr)| {
                        existing.text == prop_name.text
                    }) {
                        return Err(Diagnostic::new(
                            DiagCode::DuplicateProp,
                            prop_name.span,
                            String::from("property set twice on the same node"),
                        ));
                    }
                    self.bump(); // ':'
                    let value = self.expr()?;
                    props.push((prop_name, value));
                    let _ = self.eat(&TokenKind::Semi) || self.eat(&TokenKind::Comma);
                } else {
                    children.push(self.view_node()?);
                }
            }
        }

        // Callsite slot bodies come between the prop block and the modifiers
        // (RFC-0084 §3). Whether the node is a component that declares these
        // slots is the checker's business — the parser only shapes them.
        let slot_bodies =
            if self.peek() == &TokenKind::LBrace { self.slot_block()? } else { Vec::new() };

        let modifiers = self.modifiers()?;
        while self.peek() == &TokenKind::KwOn {
            handlers.push(self.handler()?);
        }

        Ok(ViewNode::Widget(WidgetNode {
            name,
            positional,
            props,
            children,
            slot_bodies,
            modifiers,
            handlers,
            span: start.to(self.prev_span()),
        }))
    }

    /// Zero or more `.name(args)` chains.
    fn modifiers(&mut self) -> Result<Vec<ModifierCall>, Diagnostic> {
        let mut modifiers = Vec::new();
        while self.peek() == &TokenKind::Dot
            && matches!(self.peek_at(1), TokenKind::Ident(_))
            && self.peek_at(2) == &TokenKind::LParen
        {
            self.bump(); // '.'
            let name = self.ident("a modifier name")?;
            self.bump(); // '('
            let args = self.call_args()?;
            let span = name.span.to(self.prev_span());
            modifiers.push(ModifierCall { name, args, span });
        }
        Ok(modifiers)
    }

    /// `on Trigger -> dispatch(Case(args))` / `on Trigger -> emit(expr, args…)`
    fn handler(&mut self) -> Result<HandlerDecl, Diagnostic> {
        let start = self.expect(&TokenKind::KwOn, "`on`")?;
        let trigger = self.ident("an interaction name (Tap, Change, Submit, …)")?;
        self.expect(&TokenKind::Arrow, "`->`")?;
        let action = match self.peek() {
            TokenKind::KwDispatch => {
                self.bump();
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
                HandlerAction::Dispatch { case, args }
            }
            TokenKind::KwEmit => {
                self.bump();
                self.expect(&TokenKind::LParen, "`(`")?;
                let prop = self.expr()?;
                let mut args = Vec::new();
                while self.eat(&TokenKind::Comma) {
                    args.push(self.expr()?);
                }
                self.expect(&TokenKind::RParen, "`)`")?;
                HandlerAction::Emit { prop, args }
            }
            TokenKind::KwNavigate => {
                self.bump();
                self.expect(&TokenKind::LParen, "`(`")?;
                let path = self.expr()?;
                self.expect(&TokenKind::RParen, "`)`")?;
                HandlerAction::Navigate { path }
            }
            _ => return Err(self.unexpected("`dispatch(...)`, `emit(...)`, or `navigate(...)`")),
        };
        Ok(HandlerDecl { trigger, action, span: start.to(self.prev_span()) })
    }
}
