# Agent Brief: Kernel (Neuron)

Purpose: give minimal context to solve scoped tasks without repo-wide scanning.

- Entry files: `kmain.rs`, `syscall/api.rs`, `mm/address_space.rs`, `trap.rs`, `satp.rs`.
- Invariants: syscall IDs stable; trap ABI fixed; SATP activation semantics; W^X for user mappings.
- Scope guard: do not change ABI or trap assembly without RFC/ADR.
- Typical tasks: document handler invariants, add headers, fix comments, wire tests/log markers.
- Tests: `just test-os` for UART markers; kernel selftests print `neuron vers.` and trap diagnostics.
- References: `docs/ARCHITECTURE.md` (Kernel quick reference), ADR-0001.









