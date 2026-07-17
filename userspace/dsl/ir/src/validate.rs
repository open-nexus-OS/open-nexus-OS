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
        view_node::ComponentRef(_) => {}
    }
    Ok(())
}
