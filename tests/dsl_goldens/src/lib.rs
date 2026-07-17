// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Interpreter scene goldens: `.nx` example apps compiled, mounted,
//! and rendered through the shared BGRA golden painter — plus damage-class
//! proofs (paint-only dispatches must not require re-layout).
//! OWNERS: @ui @runtime
//! STATUS: Functional (TASK-0076)
//! TEST_COVERAGE: this crate IS the coverage

pub use nexus_dsl_ir;

pub const COUNTER: &str = include_str!("../../../examples/dsl/counter/counter.nx");
pub const TODO: &str = include_str!("../../../examples/dsl/todo/todo.nx");

/// Compiles a `.nx` source to canonical `.nxir` bytes.
///
/// # Panics
/// On any compile failure — example apps must stay green.
#[must_use]
pub fn compile(source: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(source).expect("example parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "example check errors: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("example lowers").nxir
}

/// Collects the text contents of a scene in pre-order (the golden painter
/// draws no glyphs, so text-only differences are asserted structurally).
#[must_use]
pub fn texts(scene: &nexus_layout_types::LayoutNode) -> Vec<String> {
    fn walk(node: &nexus_layout_types::LayoutNode, out: &mut Vec<String>) {
        use nexus_layout_types::LayoutNode as N;
        match node {
            N::Text(text, _) => out.push(String::from(text.content.as_str())),
            N::Stack(_, _, children) | N::Grid(_, _, children) => {
                for child in children {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(scene, &mut out);
    out
}

/// The program's i18n key table (key index → symbol id) for locale sources.
#[must_use]
pub fn i18n_keys(nxir: &[u8]) -> Vec<u32> {
    let reader = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir).expect("reads");
    reader
        .root()
        .expect("root")
        .get_i18n_keys()
        .expect("keys")
        .iter()
        .map(|k| k.get_key())
        .collect()
}
