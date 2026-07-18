// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Navigation runtime (TASK-0077): routes, typed params, bounded history.
//!
//! Routes come from the IR (`Route { path, page, params }`, canonical path
//! order). Matching is segment-exact with `:name` placeholders; params parse
//! against their declared types (`Int`, id types and `Str` as text) —
//! a mismatch is a deterministic error, never a partial navigation.

use crate::store::Value;
use crate::RtError;
use alloc::{string::String, vec::Vec};
use nexus_dsl_ir::ui_ir_capnp as ir;

/// Bounded route history (push/back depth).
pub const MAX_HISTORY: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavEntry {
    /// Component index of the page.
    pub page: u32,
    /// Parsed route params, in the route's declared order.
    pub params: Vec<Value>,
}

struct RouteDef {
    /// Static segments; `None` = `:param` placeholder.
    segments: Vec<Option<String>>,
    page: u32,
    /// Param kinds in placeholder order (true = integer-typed).
    param_is_int: Vec<bool>,
}

/// The mounted program's route table + history stack.
pub struct Nav {
    routes: Vec<RouteDef>,
    stack: Vec<NavEntry>,
}

impl Nav {
    /// Builds the table from the program and enters the `/` route (or the
    /// program's entry page when no routes are declared).
    ///
    /// # Errors
    /// [`RtError::Malformed`] on unreadable IR.
    pub fn mount(root: ir::ui_program::Reader<'_>) -> Result<Self, RtError> {
        let mut routes = Vec::new();
        for route in root.get_routes().map_err(|_| RtError::Malformed)?.iter() {
            let path = route
                .get_path()
                .map_err(|_| RtError::Malformed)?
                .to_str()
                .map_err(|_| RtError::Malformed)?;
            let mut segments = Vec::new();
            for seg in path.split('/').filter(|s| !s.is_empty()) {
                if let Some(name) = seg.strip_prefix(':') {
                    let _ = name;
                    segments.push(None);
                } else {
                    segments.push(Some(String::from(seg)));
                }
            }
            let mut param_is_int = Vec::new();
            for param in route.get_params().map_err(|_| RtError::Malformed)?.iter() {
                let is_int = matches!(
                    param.get_type().map_err(|_| RtError::Malformed)?.which(),
                    Ok(ir::type_ref::Which::Int(()))
                );
                param_is_int.push(is_int);
            }
            routes.push(RouteDef { segments, page: route.get_page(), param_is_int });
        }
        let mut nav = Self { routes, stack: Vec::new() };
        let entry = nav
            .resolve("/")
            .unwrap_or(NavEntry { page: root.get_entry_page(), params: Vec::new() });
        nav.stack.push(entry);
        Ok(nav)
    }

    /// The active page.
    #[must_use]
    pub fn current(&self) -> &NavEntry {
        // The stack is never empty: mount seeds it and `back` keeps the root.
        let Some(entry) = self.stack.last() else {
            unreachable!("nav stack invariant: mount seeds it and `back` keeps the root")
        };
        entry
    }

    #[must_use]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Resolves a path against the route table (no state change).
    #[must_use]
    pub fn resolve(&self, path: &str) -> Option<NavEntry> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        'routes: for route in &self.routes {
            if route.segments.len() != segments.len() {
                continue;
            }
            let mut params = Vec::new();
            let mut param_idx = 0usize;
            for (want, have) in route.segments.iter().zip(&segments) {
                match want {
                    Some(fixed) => {
                        if fixed != have {
                            continue 'routes;
                        }
                    }
                    None => {
                        let is_int = route.param_is_int.get(param_idx).copied().unwrap_or(false);
                        param_idx += 1;
                        if is_int {
                            match have.parse::<i64>() {
                                Ok(value) => params.push(Value::Int(value)),
                                Err(_) => continue 'routes, // typed mismatch ≠ this route
                            }
                        } else {
                            params.push(Value::Str(String::from(*have)));
                        }
                    }
                }
            }
            return Some(NavEntry { page: route.page, params });
        }
        None
    }

    /// Pushes a new route onto the history.
    ///
    /// # Errors
    /// [`RtError::UnknownField`] for an unmatched path,
    /// [`RtError::Budget`] when the bounded history is full.
    pub fn push(&mut self, path: &str) -> Result<&NavEntry, RtError> {
        let entry = self.resolve(path).ok_or(RtError::UnknownField)?;
        if self.stack.len() >= MAX_HISTORY {
            return Err(RtError::Budget);
        }
        self.stack.push(entry);
        Ok(self.current())
    }

    /// Replaces the current route (no history growth).
    ///
    /// # Errors
    /// [`RtError::UnknownField`] for an unmatched path.
    pub fn replace(&mut self, path: &str) -> Result<&NavEntry, RtError> {
        let entry = self.resolve(path).ok_or(RtError::UnknownField)?;
        let Some(slot) = self.stack.last_mut() else {
            unreachable!("nav stack invariant: mount seeds it and `back` keeps the root")
        };
        *slot = entry;
        Ok(self.current())
    }

    /// Pops to the previous route. The root entry always remains.
    /// Returns whether anything changed.
    pub fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }
}
