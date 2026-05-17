# Handoff — TASK-0058 In Progress: impl done, QEMU + windowd integrated

## Architecture: single source of truth
- `layout_panel.rs` computes `LayoutResult` from `nexus-layout`
- `os_lite.rs` stores `proof_layout: Option<LayoutResult>`, computed in `new()`
- No duplicate structure: `proof_panel.rs` deleted, PROOF_PANEL_* constants backed by layout engine
- OS-compatible: `#![cfg_attr(not(test), no_std)]` on nexus-layout

Date: 2026-05-16 (production-grade: single source of truth, no duplicate structure)

## Status
- RFC-0057: In Progress (contract+impl done)
- TASK-0058: In Progress (31 tests ok, QEMU + windowd integrated)
- TASK-0059: Draft

## Crates
- nexus-layout-types: all types (no_std+alloc)
- nexus-layout: Flex+Grid engine
- nexus-shape: wrap.rs + cache.rs
- tests/ui_v3a_host: 4 JSON goldens
- windowd: proof_panel.rs + markers

## Proofs
cargo test -p nexus-layout  # 8 ok
cargo test -p nexus-shape   # 19 ok
cargo test -p ui_v3a_host   # 4 ok

## Pending
- QEMU markers
- Regression gate
- RISC-V cross-compile

## Next
Verify QEMU, set Done.
