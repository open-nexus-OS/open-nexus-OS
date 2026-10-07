// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The emitter's dependency recorder: every state read inside an expression a
//! scene site evaluates becomes a [`Dep`] of that site's damage class, so a
//! store change re-emits exactly when something on screen reads the changed
//! field.
//!
//! Completeness is the contract — a read the walk misses is a site that never
//! repaints when only that field changes. List operations are reads like any
//! other: `len($state.clips) == 0`, `take($state.items, 6)` and
//! `take(skip($state.apps, …), …)` read their list, and their argument and
//! lambda body may read more state. The walk once skipped them, and the shell
//! search's clipboard panel stayed stale after a copy (TASK-0067B): only the
//! list changed, and nothing on screen was known to read it.

use super::{Damage, Dep, EmitCtx};
use nexus_dsl_ir::ui_ir_capnp as ir;

impl EmitCtx<'_, '_> {
    /// Records every state read inside `expr` as a dependency of `damage`.
    pub(super) fn record_deps(&mut self, expr: ir::expr::Reader<'_>, damage: Damage) {
        use ir::expr::Which;
        match expr.which() {
            Ok(Which::FieldGet(Ok(get))) => {
                if let (store, Ok(path)) = (get.get_store(), get.get_path()) {
                    if !path.is_empty() {
                        // Field symbol → index resolution happens at damage
                        // time; store the *symbol* so deps survive re-emits.
                        self.deps.push(Dep { store, field: path.get(0), damage });
                    }
                }
            }
            Ok(Which::LitList(Ok(items)) | Which::RecordMake(Ok(items))) => {
                for item in items.iter() {
                    self.record_deps(item, damage);
                }
            }
            Ok(Which::LitEnum(Ok(lit))) => {
                if let Ok(payload) = lit.get_payload() {
                    for item in payload.iter() {
                        self.record_deps(item, damage);
                    }
                }
            }
            Ok(Which::UnOp(Ok(un))) => {
                if let Ok(operand) = un.get_operand() {
                    self.record_deps(operand, damage);
                }
            }
            Ok(Which::BinOp(Ok(bin))) => {
                if let Ok(lhs) = bin.get_lhs() {
                    self.record_deps(lhs, damage);
                }
                if let Ok(rhs) = bin.get_rhs() {
                    self.record_deps(rhs, damage);
                }
            }
            Ok(Which::ListOp(Ok(op))) => self.record_list_op(op, damage),
            Ok(Which::RecordGet(Ok(get))) => {
                if let Ok(base) = get.get_base() {
                    self.record_deps(base, damage);
                }
            }
            Ok(Which::FmtI18n(Ok(fmt))) => {
                if let Ok(args) = fmt.get_args() {
                    for arg in args.iter() {
                        self.record_deps(arg, damage);
                    }
                }
            }
            Ok(Which::OptionSome(Ok(inner))) => self.record_deps(inner, damage),
            _ => {}
        }
    }

    /// A list operation reads its list, its argument (an element, an index, a
    /// count) and — for `map`/`filter`/`findFirst`/`removeWhere` — whatever its
    /// lambda body reads beyond the bound element.
    fn record_list_op(&mut self, op: ir::list_op_expr::Reader<'_>, damage: Damage) {
        if op.has_base() {
            if let Ok(base) = op.get_base() {
                self.record_deps(base, damage);
            }
        }
        if op.has_arg() {
            if let Ok(arg) = op.get_arg() {
                self.record_deps(arg, damage);
            }
        }
        if op.has_lambda() {
            if let Ok(body) = op.get_lambda().and_then(|l| l.get_body()) {
                self.record_deps(body, damage);
            }
        }
    }
}
