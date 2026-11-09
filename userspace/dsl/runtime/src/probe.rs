// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host probes over a mounted program (TASK-0357 P3): every event case with a neutral payload
//! and every route with neutral params, so a smoke test can drive each case and mount each
//! route once (`tests/dsl_apps_conformance/tests/app_smoke.rs`). Test support: the OS runtime
//! never calls these.
//!
//! A neutral value is the declared type's zero (`Value::zero_of`), except a record, which gets
//! every field's own neutral value — a reducer that reads a field of a payload record must find
//! the field, not a unit.

use crate::store::Value;
use crate::Runtime;
use alloc::{string::String, vec::Vec};
use nexus_dsl_ir::ui_ir_capnp as ir;

/// How deep a record's fields are filled before a nested record stays a unit (records of
/// records are rare; a cycle would otherwise recurse).
const RECORD_DEPTH: u32 = 4;

/// One event case: its names, its ids and a neutral payload (one value per declared slot).
/// `complete` is false when a slot's type is not known to the program (a bare service result
/// type, still opaque in the IR, or a nominal id type): its value is a unit, and a reducer
/// reading it would reject it — a smoke test skips such a case and counts it.
#[derive(Debug, Clone)]
pub struct CaseProbe {
    pub event: String,
    pub case: String,
    pub ids: (u32, u32),
    pub payload: Vec<Value>,
    pub complete: bool,
}

impl Runtime<'_> {
    /// Every event case of the program, in declaration order, with a neutral payload.
    #[must_use]
    pub fn case_probes(&self) -> Vec<CaseProbe> {
        let mut out = Vec::new();
        let reader = self.reader();
        let Ok(root) = reader.root() else { return out };
        let (Ok(events), types) = (root.get_events(), root.get_types().ok()) else { return out };
        let symbols = self.symbols();
        let name = |sym: u32| symbols.get(sym as usize).cloned().unwrap_or_default();
        for (e, decl) in events.iter().enumerate() {
            let Ok(cases) = decl.get_cases() else { continue };
            for (c, case) in cases.iter().enumerate() {
                let tys: Vec<_> =
                    case.get_payload().map(|tys| tys.iter().collect()).unwrap_or_default();
                let complete = tys.iter().all(|ty| known(*ty));
                let payload = tys.iter().map(|ty| neutral(*ty, types, 0)).collect();
                out.push(CaseProbe {
                    event: name(decl.get_name()),
                    case: name(case.get_name()),
                    ids: (e as u32, c as u32),
                    payload,
                    complete,
                });
            }
        }
        out
    }

    /// Every route path of the program, its `:param` segments filled with `0` (an `Int` parses
    /// it, a `Str` or an id type takes it as text).
    #[must_use]
    pub fn route_probes(&self) -> Vec<String> {
        let reader = self.reader();
        let Ok(root) = reader.root() else { return Vec::new() };
        let Ok(routes) = root.get_routes() else { return Vec::new() };
        routes
            .iter()
            .filter_map(|route| route.get_path().ok()?.to_str().ok().map(String::from))
            .map(|path| {
                let segments: Vec<String> = path
                    .split('/')
                    .map(|s| if s.starts_with(':') { String::from("0") } else { String::from(s) })
                    .collect();
                segments.join("/")
            })
            .map(|path| if path.is_empty() { String::from("/") } else { path })
            .collect()
    }
}

fn neutral(
    ty: ir::type_ref::Reader<'_>,
    types: Option<capnp::struct_list::Reader<'_, ir::type_def::Owned>>,
    depth: u32,
) -> Value {
    if let (Ok(ir::type_ref::Which::Record(index)), Some(types)) = (ty.which(), types) {
        if depth < RECORD_DEPTH && index < types.len() {
            if let Ok(ir::type_def::Which::RecordDef(Ok(def))) = types.get(index).which() {
                if let Ok(fields) = def.get_fields() {
                    let mut record: Vec<(u32, Value)> = fields
                        .iter()
                        .filter_map(|f| {
                            Some((
                                f.get_name(),
                                neutral(f.get_type().ok()?, Some(types), depth + 1),
                            ))
                        })
                        .collect();
                    record.sort_by_key(|(sym, _)| *sym);
                    return Value::Record(record);
                }
            }
        }
    }
    Value::zero_of(ty)
}

/// Whether the program knows the type well enough for a neutral value: not a bare service
/// result type (opaque in the IR) or nominal id. A list is always known — its neutral value is
/// empty, whatever its elements.
fn known(ty: ir::type_ref::Reader<'_>) -> bool {
    use ir::type_ref::Which;
    !matches!(ty.which(), Ok(Which::Opaque(())) | Ok(Which::IdType(_)) | Err(_))
}
