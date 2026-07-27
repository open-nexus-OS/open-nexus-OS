// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slot emission (RFC-0084): where a caller's content lands inside a
//! component's view.
//!
//! Two laws, both enforced here rather than hoped for:
//!
//! **Caller scope.** A slot body is lexically part of the caller, so before
//! emitting one the frame restores the caller's `params` AND a snapshot of the
//! caller's `locals`. The snapshot is what makes this airtight rather than
//! usually-right: `locals` are shared across the component boundary (see the
//! `ComponentRef` arm in the parent module), so a `for` inside the callee can
//! clobber a caller loop binding — and a slot body is emitted *after* that
//! could have happened.
//!
//! **Splice, not wrap.** A bound slot contributes its N nodes at the
//! placeholder's real position among its parent's children. Wrapping them
//! would reset the flex context and break `.grow(1)`/`.width(240)` on the
//! receiving region — the same hazard the transparent-branch and ForEach-splice
//! paths already encode.
//!
//! Forwarding is out in v1: entering a body clears the frame, so a `Slot`
//! nested in one finds nothing. The checker reports it (`NX0411`) so the
//! author sees an error rather than silence.

use super::EmitCtx;
use crate::store::Value;
use crate::RtError;
use alloc::vec::Vec;
use nexus_dsl_ir::ui_ir_capnp as ir;
use nexus_layout_types::LayoutNode;

/// The slot bodies one component instance received, plus the caller scope they
/// must be emitted in.
pub(crate) struct SlotFrame<'a, 'p> {
    pub args: capnp::struct_list::Reader<'p, ir::slot_arg::Owned>,
    /// The CALLER's params (`$props` inside a body resolves against these).
    pub params: &'a [Value],
    /// Snapshot of the CALLER's locals at the callsite.
    pub locals: Vec<Option<Value>>,
}

impl<'p> SlotFrame<'_, 'p> {
    /// The body bound to `slot`, or `None` when the caller left it unbound.
    fn body(&self, slot: u16) -> Option<capnp::struct_list::Reader<'p, ir::view_node::Owned>> {
        self.args.iter().find(|arg| arg.get_slot() == slot).and_then(|arg| arg.get_body().ok())
    }
}

/// Emits the body bound to `slot` DIRECTLY into `out`, each node path-tagged
/// at its real position among the receiving widget's children
/// (`prefix ++ [base + out.len()]`) — the same tagging `emit_for_each_items`
/// uses, so handlers inside a slot body resolve to the right box ids.
///
/// An unbound slot (or a component emitted outside any reference) contributes
/// ZERO nodes. Not an empty box: that is how a scaffold hides a region.
pub(crate) fn emit_slot_items(
    ctx: &mut EmitCtx<'_, '_>,
    slot: u16,
    prefix: &[u32],
    base: u32,
    out: &mut Vec<LayoutNode>,
) -> Result<(), RtError> {
    let Some(frame) = ctx.slots else { return Ok(()) };
    let Some(body) = frame.body(slot) else { return Ok(()) };

    // Enter the caller's scope.
    let saved_locals: Vec<Option<Value>> = ctx.locals.to_vec();
    let restore_len = saved_locals.len().min(frame.locals.len());
    ctx.locals[..restore_len].clone_from_slice(&frame.locals[..restore_len]);
    let saved_params = core::mem::replace(&mut ctx.params, frame.params);
    let saved_slots = ctx.slots.take();

    let mut result = Ok(());
    for node in body.iter() {
        for &seg in prefix {
            ctx.path.push(seg);
        }
        ctx.path.push(base + out.len() as u32);
        match super::emit_view(ctx, node) {
            Ok(emitted) => out.push(emitted),
            Err(err) => result = Err(err),
        }
        ctx.path.pop();
        for _ in prefix {
            ctx.path.pop();
        }
        if result.is_err() {
            break;
        }
    }

    // Leave — unconditionally, so an error on one node cannot strand the
    // callee's view in the caller's scope.
    ctx.params = saved_params;
    ctx.slots = saved_slots;
    ctx.locals[..restore_len].clone_from_slice(&saved_locals[..restore_len]);
    result
}
