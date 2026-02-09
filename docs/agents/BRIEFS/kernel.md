# Agent Brief: Kernel (Neuron)

Purpose: give minimal context to solve scoped tasks without repo-wide scanning.

- Entry files: `source/kernel/neuron/src/core/kmain.rs`, `source/kernel/neuron/src/syscall/api.rs`,
  `source/kernel/neuron/src/mm/address_space.rs`, `source/kernel/neuron/src/core/trap.rs`,
  `source/kernel/neuron/src/mm/satp.rs`.
- Invariants: syscall IDs stable; trap ABI fixed; SATP activation semantics; W^X for user mappings.
- Scope guard: do not change ABI or trap assembly without RFC/ADR.
- Typical tasks: document handler invariants, add headers, fix comments, wire tests/log markers.
- Tests: `just test-os` for UART markers; kernel selftests print `neuron vers.` and trap diagnostics.
- References: `docs/ARCHITECTURE.md` (Kernel quick reference), ADR-0001.

Default expectations:

- Implementations must be real (no “fake ok” logs). If something is stubbed, return `-errno` /
  `Unsupported` and label it.
- For kernel behavior changes, always provide proof via `just test-os` (or a narrower kernel test
  if available) unless explicitly blocked.
