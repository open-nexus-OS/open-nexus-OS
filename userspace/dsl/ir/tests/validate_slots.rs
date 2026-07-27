// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Fail-closed validation of the RFC-0084 slot index spaces.
//!
//! The compiler cannot produce any of the programs below — that is the point.
//! `validate_program` runs before anything trusts a payload, so every index a
//! hostile or corrupt `.nxir` could carry has to be bounds-checked there, not
//! at the point of use. One of these cases (`ComponentRef.component` out of
//! range) closes a hole that predates slots: the check was a documented no-op
//! and the runtime called `components.get(idx)` unguarded.

// reason: test harness — a malformed fixture must panic loudly, not be
// silently propagated.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_ir::{ui_ir_capnp as ir, IrError};

/// How the fixture program deviates from the well-formed one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tamper {
    /// A valid program: one component with two slots, one page binding both.
    None,
    /// `SlotRef.slot` past the enclosing component's slot list.
    PlaceholderOutOfRange,
    /// `SlotArg.slot` past the callee's slot list.
    ArgOutOfRange,
    /// `ComponentRef.slots` in descending order (not the canonical form).
    ArgsDescending,
    /// `ComponentRef.component` past the component table.
    ComponentOutOfRange,
    /// A slot body wider than `maxChildren`.
    BodyOverBudget,
}

/// Symbols, sorted and unique as `check_symbols` demands.
const SYMBOLS: &[&str] = &["Frame", "P", "Stack", "content", "sidebar"];
const SYM_FRAME: u32 = 0;
const SYM_PAGE: u32 = 1;
const SYM_STACK: u32 = 2;
const SYM_CONTENT: u32 = 3;
const SYM_SIDEBAR: u32 = 4;

const MAX_CHILDREN: u32 = 4;

/// Builds the fixture with a zeroed `programHash`, then re-emits it with the
/// real hash — the same two-pass shape the compiler uses, so `validate_program`
/// reaches the structural checks instead of stopping at the digest.
fn fixture(tamper: Tamper) -> Vec<u8> {
    let zero = [0u8; 32];
    let first = build(tamper, &zero);
    let reader =
        nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&first).expect("fixture re-reads");
    let hash = nexus_dsl_ir::hashing::compute_program_hash(reader.root().expect("root"))
        .expect("fixture hashes");
    build(tamper, &hash)
}

fn build(tamper: Tamper, hash: &[u8; 32]) -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    {
        let mut program = message.init_root::<ir::ui_program::Builder<'_>>();
        program.set_schema_version_major(nexus_dsl_ir::SCHEMA_MAJOR);
        program.set_schema_version_minor(nexus_dsl_ir::SCHEMA_MINOR);
        program.set_program_hash(hash);
        program.set_source_digest(&[0u8; 32]);
        program.set_entry_page(1);
        {
            let mut symbols = program.reborrow().init_symbols(SYMBOLS.len() as u32);
            for (i, s) in SYMBOLS.iter().enumerate() {
                symbols.set(i as u32, capnp::text::Reader::from(*s));
            }
        }
        {
            let mut budgets = program.reborrow().init_budgets();
            budgets.set_max_view_nodes(4096);
            budgets.set_max_expr_nodes(1024);
            budgets.set_max_list_len(1024);
            budgets.set_max_str_len(4096);
            budgets.set_max_effect_steps(16);
            budgets.set_max_locals(32);
            budgets.set_max_children(MAX_CHILDREN);
        }

        let mut components = program.reborrow().init_components(2);

        // [0] `Frame`: two slots, view = Stack { Slot sidebar }.
        {
            let mut frame = components.reborrow().get(0);
            frame.set_name(SYM_FRAME);
            frame.set_is_page(false);
            frame.reborrow().init_props(0);
            {
                let mut slots = frame.reborrow().init_slots(2);
                slots.set(0, SYM_SIDEBAR);
                slots.set(1, SYM_CONTENT);
            }
            let mut widget = frame.init_view().init_widget();
            widget.set_kind(SYM_STACK);
            widget.reborrow().init_props(0);
            let mut children = widget.init_children(1);
            let placeholder = if tamper == Tamper::PlaceholderOutOfRange { 7 } else { 0 };
            children.reborrow().get(0).init_slot().set_slot(placeholder);
        }

        // [1] `P`: a page whose whole view is the reference to `Frame`.
        {
            let mut page = components.reborrow().get(1);
            page.set_name(SYM_PAGE);
            page.set_is_page(true);
            page.reborrow().init_props(0);
            page.reborrow().init_slots(0);
            let mut component_ref = page.init_view().init_component_ref();
            component_ref.set_component(if tamper == Tamper::ComponentOutOfRange { 9 } else { 0 });
            component_ref.reborrow().init_args(0);

            let bound: [u16; 2] = if tamper == Tamper::ArgsDescending { [1, 0] } else { [0, 1] };
            let bound: [u16; 2] = if tamper == Tamper::ArgOutOfRange { [0, 5] } else { bound };
            let mut slots = component_ref.init_slots(2);
            for (i, slot) in bound.iter().enumerate() {
                let mut arg = slots.reborrow().get(i as u32);
                arg.set_slot(*slot);
                let width = if tamper == Tamper::BodyOverBudget { MAX_CHILDREN + 1 } else { 1 };
                let mut body = arg.init_body(width);
                for j in 0..width {
                    let mut child = body.reborrow().get(j).init_widget();
                    child.set_kind(SYM_STACK);
                    child.reborrow().init_props(0);
                    child.init_children(0);
                }
            }
        }

        program.reborrow().init_i18n_keys(0);
    }

    // Canonical single segment, as every consumer expects.
    let words: usize =
        message.get_segments_for_output().iter().map(|segment| segment.len() / 8).sum();
    let mut canonical = capnp::message::Builder::new(
        capnp::message::HeapAllocator::new()
            .first_segment_words(u32::try_from(words + 64).unwrap_or(u32::MAX)),
    );
    canonical
        .set_root_canonical(
            message.get_root_as_reader::<ir::ui_program::Reader<'_>>().expect("reread"),
        )
        .expect("canonicalize");
    canonical.get_segments_for_output()[0].to_vec()
}

fn validate(tamper: Tamper) -> Result<(), IrError> {
    let bytes = fixture(tamper);
    let reader =
        nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&bytes).expect("fixture reads");
    nexus_dsl_ir::validate::validate_program(reader.root().expect("root"))
}

#[test]
fn well_formed_slot_program_validates() {
    validate(Tamper::None).expect("the untampered fixture is valid");
}

#[test]
fn rejects_placeholder_past_the_enclosing_slot_list() {
    assert_eq!(validate(Tamper::PlaceholderOutOfRange), Err(IrError::DanglingRef));
}

#[test]
fn rejects_slot_arg_past_the_callee_slot_list() {
    assert_eq!(validate(Tamper::ArgOutOfRange), Err(IrError::DanglingRef));
}

#[test]
fn rejects_non_ascending_slot_args() {
    // Canonical form is ascending; anything else did not come from this
    // toolchain, so it is malformed rather than merely unusual.
    assert_eq!(validate(Tamper::ArgsDescending), Err(IrError::Malformed));
}

#[test]
fn rejects_component_ref_past_the_component_table() {
    // The pre-existing hole: never checked before RFC-0084.
    assert_eq!(validate(Tamper::ComponentOutOfRange), Err(IrError::DanglingRef));
}

#[test]
fn rejects_slot_bodies_over_the_children_budget() {
    // Slot bodies used to escape the traversal budgets entirely, because the
    // ComponentRef arm of `count_view_nodes` did not recurse.
    assert_eq!(validate(Tamper::BodyOverBudget), Err(IrError::BudgetExceeded));
}
