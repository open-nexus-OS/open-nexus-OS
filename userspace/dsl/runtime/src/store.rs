// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Runtime values + store state.
//!
//! Values mirror the IR type vocabulary (docs/dev/dsl/types.md). State writes
//! go through [`StoreState::set_path`], which compares before writing and
//! marks the change bitmap — the dispatch path never snapshots state, so a
//! scalar update is allocation-free.

use crate::RtError;
use alloc::{string::String, vec, vec::Vec};
use nexus_dsl_ir::ui_ir_capnp as ir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Unit,
    Bool(bool),
    Int(i64),
    /// Raw Q32.32.
    Fx(i64),
    Str(String),
    List(Vec<Value>),
    /// An event/enum case value.
    Enum { event: u32, case: u32, payload: Vec<Value> },
    /// Named-field record (field symbol id → value), field-sorted.
    Record(Vec<(u32, Value)>),
}

impl Value {
    /// The neutral value for a declared type (used when a field has no
    /// default).
    pub(crate) fn zero_of(ty: ir::type_ref::Reader<'_>) -> Value {
        use ir::type_ref::Which;
        match ty.which() {
            Ok(Which::Bool(())) => Value::Bool(false),
            Ok(Which::Int(())) => Value::Int(0),
            Ok(Which::Fx(())) => Value::Fx(0),
            Ok(Which::Str(_)) => Value::Str(String::new()),
            Ok(Which::List(_)) => Value::List(Vec::new()),
            _ => Value::Unit,
        }
    }

    /// Stable bytes for keyed identity (collection `.key(expr)` values).
    pub(crate) fn key_bytes(&self, out: &mut Vec<u8>) {
        match self {
            Value::Unit => out.push(0),
            Value::Bool(b) => out.extend_from_slice(&[1, u8::from(*b)]),
            Value::Int(i) => {
                out.push(2);
                out.extend_from_slice(&i.to_le_bytes());
            }
            Value::Fx(f) => {
                out.push(3);
                out.extend_from_slice(&f.to_le_bytes());
            }
            Value::Str(s) => {
                out.push(4);
                out.extend_from_slice(&(s.len() as u32).to_le_bytes());
                out.extend_from_slice(s.as_bytes());
            }
            Value::List(items) => {
                out.push(5);
                for item in items {
                    item.key_bytes(out);
                }
            }
            Value::Enum { event, case, payload } => {
                out.push(6);
                out.extend_from_slice(&event.to_le_bytes());
                out.extend_from_slice(&case.to_le_bytes());
                for item in payload {
                    item.key_bytes(out);
                }
            }
            Value::Record(fields) => {
                out.push(7);
                for (sym, value) in fields {
                    out.extend_from_slice(&sym.to_le_bytes());
                    value.key_bytes(out);
                }
            }
        }
    }
}

/// One store's live state.
pub struct StoreState {
    /// Field values, in IR field order.
    pub(crate) fields: Vec<Value>,
    /// Field symbol id per index (IR order), for path resolution.
    pub(crate) field_syms: Vec<u32>,
    /// Change bitmap since the last [`StoreState::take_changes`].
    changed: Vec<bool>,
}

impl StoreState {
    pub(crate) fn new(fields: Vec<Value>, field_syms: Vec<u32>) -> Self {
        let len = fields.len();
        Self { fields, field_syms, changed: vec![false; len] }
    }

    /// Field symbol id for an index (dep matching).
    #[must_use]
    pub fn field_sym(&self, index: usize) -> Option<u32> {
        self.field_syms.get(index).copied()
    }

    pub(crate) fn field_index(&self, sym: u32) -> Result<usize, RtError> {
        self.field_syms.iter().position(|&s| s == sym).ok_or(RtError::UnknownField)
    }

    pub(crate) fn get(&self, index: usize) -> Result<&Value, RtError> {
        self.fields.get(index).ok_or(RtError::UnknownField)
    }

    /// Writes through a field path (`field` or `field.recordField…`),
    /// comparing first: an equal write neither allocates nor marks dirty.
    pub(crate) fn set_path(&mut self, path: &[u32], value: Value) -> Result<(), RtError> {
        let (&first, rest) = path.split_first().ok_or(RtError::UnknownField)?;
        let index = self.field_index(first)?;
        let slot = self.fields.get_mut(index).ok_or(RtError::UnknownField)?;
        let target = resolve_record_path(slot, rest)?;
        if *target != value {
            *target = value;
            self.changed[index] = true;
        }
        Ok(())
    }

    /// Drains the change bitmap, invoking `f` per changed field index.
    pub(crate) fn take_changes(&mut self, mut f: impl FnMut(usize)) {
        for (index, flag) in self.changed.iter_mut().enumerate() {
            if *flag {
                *flag = false;
                f(index);
            }
        }
    }
}

fn resolve_record_path<'v>(
    mut value: &'v mut Value,
    path: &[u32],
) -> Result<&'v mut Value, RtError> {
    for &field in path {
        match value {
            Value::Record(fields) => {
                value = fields
                    .iter_mut()
                    .find(|(sym, _)| *sym == field)
                    .map(|(_, v)| v)
                    .ok_or(RtError::UnknownField)?;
            }
            _ => return Err(RtError::TypeMismatch),
        }
    }
    Ok(value)
}
