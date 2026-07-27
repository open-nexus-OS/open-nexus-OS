// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Structural validation of a loaded program (fail-closed).
//!
//! Runs once at load/mount time — before anything trusts the payload. The
//! validator and the runtime must agree exactly: the runtime never tolerates
//! what the validator rejects, and never re-checks what the validator proved.
//!
//! Checked here (v1.0):
//! - schema major gate (also enforced by [`crate::read::ProgramReader`])
//! - digest field lengths + `programHash` recomputation
//! - symbol table canonicality (sorted, unique)
//! - cross-reference bounds for every u32 index table
//! - program-declared budgets against the program's own contents
//!
//! Expression re-typechecking tightens over the task phases; the entry point
//! stays `validate_program`.

use crate::{
    ui_ir_capnp::{ui_program, view_node},
    IrError, DIGEST_LEN,
};

/// Validates a program root. Cheap relative to mount; call exactly once.
///
/// # Errors
/// The first violated invariant, as a stable [`IrError`].
pub fn validate_program(root: ui_program::Reader<'_>) -> Result<(), IrError> {
    if root.get_schema_version_major() != crate::SCHEMA_MAJOR {
        return Err(IrError::UnsupportedMajor);
    }
    let source_digest = root.get_source_digest().map_err(|_| IrError::Malformed)?;
    if source_digest.len() != DIGEST_LEN {
        return Err(IrError::BadDigest);
    }
    // Hash recomputation is feature-gated: build-embedded payloads run inside
    // their trust boundary (the binary), and the sha2 + capnp-writer code it
    // pulls in is measurable text in size-tight services. Fetched payloads
    // (app-host GET_PAYLOAD, CLI) build with `hash-verify` ON.
    #[cfg(feature = "hash-verify")]
    crate::hashing::verify_program_hash(root)?;
    check_symbols(root)?;
    check_refs(root)?;
    check_view_refs(root)?;
    check_budgets(root)?;
    Ok(())
}

fn check_symbols(root: ui_program::Reader<'_>) -> Result<(), IrError> {
    let symbols = root.get_symbols().map_err(|_| IrError::Malformed)?;
    let mut prev: Option<&str> = None;
    for symbol in symbols.iter() {
        let text =
            symbol.map_err(|_| IrError::Malformed)?.to_str().map_err(|_| IrError::Malformed)?;
        if let Some(p) = prev {
            if p >= text {
                return Err(IrError::SymbolsNotCanonical);
            }
        }
        prev = Some(text);
    }
    Ok(())
}

/// Bounds-checks the coarse cross-reference tables (index → table length).
fn check_refs(root: ui_program::Reader<'_>) -> Result<(), IrError> {
    let symbol_count = root.get_symbols().map_err(|_| IrError::Malformed)?.len();
    let store_count = root.get_stores().map_err(|_| IrError::Malformed)?.len();
    let event_count = root.get_events().map_err(|_| IrError::Malformed)?.len();
    let component_count = root.get_components().map_err(|_| IrError::Malformed)?.len();

    let in_symbols = |id: u32| if id < symbol_count { Ok(()) } else { Err(IrError::DanglingRef) };

    for store in root.get_stores().map_err(|_| IrError::Malformed)?.iter() {
        in_symbols(store.get_name())?;
        for field in store.get_fields().map_err(|_| IrError::Malformed)?.iter() {
            in_symbols(field.get_name())?;
        }
    }
    for event in root.get_events().map_err(|_| IrError::Malformed)?.iter() {
        in_symbols(event.get_name())?;
        for case in event.get_cases().map_err(|_| IrError::Malformed)?.iter() {
            in_symbols(case.get_name())?;
        }
    }
    for reducer in root.get_reducers().map_err(|_| IrError::Malformed)?.iter() {
        if reducer.get_store() >= store_count || reducer.get_event() >= event_count {
            return Err(IrError::DanglingRef);
        }
    }
    for effect in root.get_effects().map_err(|_| IrError::Malformed)?.iter() {
        if effect.get_event() >= event_count {
            return Err(IrError::DanglingRef);
        }
    }
    for route in root.get_routes().map_err(|_| IrError::Malformed)?.iter() {
        if route.get_page() >= component_count {
            return Err(IrError::DanglingRef);
        }
    }
    let entry = root.get_entry_page();
    if component_count > 0 && entry >= component_count {
        return Err(IrError::DanglingRef);
    }
    Ok(())
}

fn check_budgets(root: ui_program::Reader<'_>) -> Result<(), IrError> {
    let budgets = root.get_budgets().map_err(|_| IrError::Malformed)?;
    let max_view_nodes = budgets.get_max_view_nodes();
    let max_children = budgets.get_max_children();
    if max_view_nodes == 0 || max_children == 0 {
        return Err(IrError::BudgetExceeded);
    }
    let mut total: u32 = 0;
    for component in root.get_components().map_err(|_| IrError::Malformed)?.iter() {
        count_view_nodes(
            component.get_view().map_err(|_| IrError::Malformed)?,
            max_children,
            &mut total,
        )?;
        if total > max_view_nodes {
            return Err(IrError::BudgetExceeded);
        }
    }
    Ok(())
}

fn count_view_nodes(
    node: view_node::Reader<'_>,
    max_children: u32,
    total: &mut u32,
) -> Result<(), IrError> {
    *total = total.saturating_add(1);
    match node.which().map_err(|_| IrError::Malformed)? {
        view_node::Widget(widget) => {
            let widget = widget.map_err(|_| IrError::Malformed)?;
            let children = widget.get_children().map_err(|_| IrError::Malformed)?;
            if children.len() > max_children {
                return Err(IrError::BudgetExceeded);
            }
            for child in children.iter() {
                count_view_nodes(child, max_children, total)?;
            }
        }
        view_node::ForEach(for_each) => {
            let for_each = for_each.map_err(|_| IrError::Malformed)?;
            count_view_nodes(
                for_each.get_template().map_err(|_| IrError::Malformed)?,
                max_children,
                total,
            )?;
        }
        view_node::Branch(branch) => {
            let branch = branch.map_err(|_| IrError::Malformed)?;
            for arm in branch.get_arms().map_err(|_| IrError::Malformed)?.iter() {
                for child in arm.get_body().map_err(|_| IrError::Malformed)?.iter() {
                    count_view_nodes(child, max_children, total)?;
                }
            }
            for child in branch.get_else_body().map_err(|_| IrError::Malformed)?.iter() {
                count_view_nodes(child, max_children, total)?;
            }
        }
        view_node::ComponentRef(component_ref) => {
            // Slot bodies live INSIDE the ComponentRef. Before RFC-0084 this
            // arm was an honest no-op (a ref carried no nodes); now it must
            // recurse, or a body could carry unbounded nodes past the budget.
            let component_ref = component_ref.map_err(|_| IrError::Malformed)?;
            for arg in component_ref.get_slots().map_err(|_| IrError::Malformed)?.iter() {
                let body = arg.get_body().map_err(|_| IrError::Malformed)?;
                if body.len() > max_children {
                    return Err(IrError::BudgetExceeded);
                }
                for child in body.iter() {
                    count_view_nodes(child, max_children, total)?;
                }
            }
        }
        // A leaf: its body is counted at the callsite that bound it.
        view_node::Slot(_) => {}
    }
    Ok(())
}

/// Walks every view node for the index spaces `check_refs` cannot reach:
/// component references and slots (RFC-0084 §7).
///
/// `ComponentRef.component` was NEVER bounds-checked before this — the arm in
/// [`count_view_nodes`] was a documented no-op and the runtime called
/// `components.get(idx)` unguarded. That fail-open hole closes here.
fn check_view_refs(root: ui_program::Reader<'_>) -> Result<(), IrError> {
    let symbol_count = root.get_symbols().map_err(|_| IrError::Malformed)?.len();
    let components = root.get_components().map_err(|_| IrError::Malformed)?;
    let component_count = components.len();

    // Slot counts per component, so a SlotArg can be checked against the
    // CALLEE and a SlotRef against its ENCLOSING component.
    let slot_count = |index: u32| -> Result<u32, IrError> {
        if index >= component_count {
            return Err(IrError::DanglingRef);
        }
        Ok(components.get(index).get_slots().map_err(|_| IrError::Malformed)?.len())
    };

    for index in 0..component_count {
        let component = components.get(index);
        for slot in component.get_slots().map_err(|_| IrError::Malformed)?.iter() {
            if slot >= symbol_count {
                return Err(IrError::DanglingRef);
            }
        }
        let own_slots = component.get_slots().map_err(|_| IrError::Malformed)?.len();
        walk_view_refs(
            component.get_view().map_err(|_| IrError::Malformed)?,
            own_slots,
            &slot_count,
        )?;
    }
    Ok(())
}

fn walk_view_refs(
    node: view_node::Reader<'_>,
    own_slots: u32,
    slot_count: &dyn Fn(u32) -> Result<u32, IrError>,
) -> Result<(), IrError> {
    match node.which().map_err(|_| IrError::Malformed)? {
        view_node::Widget(widget) => {
            let widget = widget.map_err(|_| IrError::Malformed)?;
            for child in widget.get_children().map_err(|_| IrError::Malformed)?.iter() {
                walk_view_refs(child, own_slots, slot_count)?;
            }
        }
        view_node::ForEach(for_each) => {
            let for_each = for_each.map_err(|_| IrError::Malformed)?;
            walk_view_refs(
                for_each.get_template().map_err(|_| IrError::Malformed)?,
                own_slots,
                slot_count,
            )?;
        }
        view_node::Branch(branch) => {
            let branch = branch.map_err(|_| IrError::Malformed)?;
            for arm in branch.get_arms().map_err(|_| IrError::Malformed)?.iter() {
                for child in arm.get_body().map_err(|_| IrError::Malformed)?.iter() {
                    walk_view_refs(child, own_slots, slot_count)?;
                }
            }
            for child in branch.get_else_body().map_err(|_| IrError::Malformed)?.iter() {
                walk_view_refs(child, own_slots, slot_count)?;
            }
        }
        view_node::ComponentRef(component_ref) => {
            let component_ref = component_ref.map_err(|_| IrError::Malformed)?;
            let callee = component_ref.get_component();
            let callee_slots = slot_count(callee)?;
            let mut prev: Option<u16> = None;
            for arg in component_ref.get_slots().map_err(|_| IrError::Malformed)?.iter() {
                let slot = arg.get_slot();
                if u32::from(slot) >= callee_slots {
                    return Err(IrError::DanglingRef);
                }
                // Canonical form is strictly ascending; anything else is not
                // a program this toolchain produced.
                if prev.is_some_and(|p| p >= slot) {
                    return Err(IrError::Malformed);
                }
                prev = Some(slot);
                // A body is the CALLER's code, so its placeholders (there are
                // none — the checker forbids forwarding) and nested refs are
                // validated in the caller's slot space, not the callee's.
                for child in arg.get_body().map_err(|_| IrError::Malformed)?.iter() {
                    walk_view_refs(child, own_slots, slot_count)?;
                }
            }
        }
        view_node::Slot(slot_ref) => {
            let slot_ref = slot_ref.map_err(|_| IrError::Malformed)?;
            if u32::from(slot_ref.get_slot()) >= own_slots {
                return Err(IrError::DanglingRef);
            }
        }
    }
    Ok(())
}
