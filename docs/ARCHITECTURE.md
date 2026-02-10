# Userspace vs. services layering

The Open Nexus workspace enforces a host-first development workflow. Domain
logic lives in `userspace/<crate>` libraries that expose safe, testable APIs
behind mutually exclusive `nexus_env="host"` and `nexus_env="os"` configurations.
Each `userspace` crate must compile with exactly one environment selected via
`RUSTFLAGS='--cfg nexus_env="..."'` and forbids all unsafe code.

Project-level vision lens (architecture/security/performance direction):

- `docs/agents/VISION.md`

Daemons under `source/services/<name>d` are thin adapters. They register with
`samgr`, expose IDL bindings, and forward requests into the corresponding
userspace crate compiled with `nexus_env="os"`.

The `tools/arch-check` utility fails CI when a userspace crate depends on the
kernel, HAL, samgrd, nexus-abi, or any crate under `source/services/`. This
preserves the separation between host-tested logic and system wiring.

## Kernel bring-up guardrails (current increment)

- Strict stage policy: early boot uses raw UART only, no heavy formatting/alloc before selftests.
- Minimal `spawn`/`cap_transfer` implemented to exercise task control and rights derivation.
- Selftests run on a private stack with canaries and masked timer IRQs; UART markers allow deterministic CI exit.
- Optional trap symbolization (`trap_symbols`) prints nearest function for `sepc` without runtime cost when disabled.
- Boot gates v1 (RFC-0013): readiness contract (`init: up` vs `<svc>: ready`), spawn failure reasons, and resource/leak sentinel markers are enforced by the QEMU harness.
- SMP v1 baseline (TASK-0012 / RFC-0021): strict anti-fake IPI proof chain is enforced in selftests and marker gating (`REQUIRE_SMP=1`) for SMP runs.
- Memory pressure can still surface as `ALLOC-FAIL` until `TASK-0228` (cooperative `oomd`) lands; use boot-gate markers to diagnose quickly.

## Control-plane IDL strategy

Cap'n Proto schemas live exclusively in userspace under `tools/nexus-idl` with
generated Rust emitted by `userspace/nexus-idl-runtime`. Kernel components never
parse Cap'n Proto payloads; they only shuttle handles or VMOs referenced in the
metadata. Service daemons link `nexus-idl-runtime` to translate Cap'n Proto
messages into safe userspace library calls while bulk payloads continue to move
out-of-band via VMOs and `map()`.

## Kernel quick reference (for devs and agents)

- Entry: `kmain()` brings up HAL, Sv39 `AddressSpaceManager`, installs syscall table, starts `Scheduler` and `ipc::Router`.
- SATP: kernel address space activated early; RISC-V trampoline via `satp_switch_island`.
- Idle loop: drives cooperative scheduling via `SYSCALL_YIELD`.
- SMP proofs: dual-mode deterministic ladder with explicit SMP marker gate (`SMP=2 REQUIRE_SMP=1 ...` and `SMP=1 ...`).
- Syscalls (os-lite): `yield`, `nsec`, `send/recv`, `map`, `vmo_create/write`, `spawn`, `cap_transfer`, `as_create/map`, `exit`, `wait`, `debug_putc`.
- Entry points: `source/kernel/neuron/src/core/kmain.rs`, `source/kernel/neuron/src/syscall/api.rs`,
  `source/kernel/neuron/src/mm/address_space.rs`, `source/kernel/neuron/src/core/trap.rs`,
  `source/kernel/neuron/src/mm/satp.rs`.
- Don't touch without RFC/ADR: syscall IDs/ABI, trap prologue/epilogue, kernel memory map/SATP assumptions.

## Observability (userspace)

- **`logd`**: Bounded RAM journal for structured logs (APPEND/QUERY/STATS); see `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`.
- **`nexus-log`**: Unified logging facade used by all services; see `docs/rfcs/RFC-0003-unified-logging.md`.
- **Crash reporting**: `execd` emits crash markers and appends structured events to `logd` on non-zero exits.
- **Core services integrated**: `samgrd`, `bundlemgrd`, `policyd`, `dsoftbusd` emit structured logs via `nexus-log`.
- **Proof**: `cargo test -p logd`, `cargo test -p nexus-log`, `RUN_UNTIL_MARKER=1 just test-os` (5 markers green as of 2026-01-14).

## Policy Authority + Audit (TASK-0008)

- **`policyd`**: Single policy authority for capability-based access control.
- **`nexus-sel`**: Policy evaluation library with service-id based lookups.
- **Deny-by-default**: Operations without explicit policy allow are rejected.
- **Channel-bound identity**: Policy decisions bind to kernel-provided `sender_service_id` (unforgeable).
- **Audit trail**: All allow/deny decisions logged via `logd`.
- **Policy-gated operations**: `keystored` signing requires `crypto.sign` capability.
- **Proof**: `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os` (policy markers green as of 2026-01-25).
- **RFC**: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`.

## Device identity keys (TASK-0008B)

- **OS entropy path**: virtio-rng MMIO → `rngd` (single entropy authority) → `keystored` device keygen
- **Security invariants**: no entropy/private key logging; pubkey export only; deny-by-default via `policyd`; audit via `logd`
- **Contract**: `docs/rfcs/RFC-0016-device-identity-keys-v1.md`
- **Narrative**: `docs/security/identity-and-sessions.md` and `docs/architecture/13-identity-and-keystore.md`

Canonical: this is the single architecture page. For deeper details, read the source files listed above.

## Architecture index (start here for deeper docs)

For onboarding-friendly architecture notes and stable entrypoints into subsystem docs, see:

- `docs/architecture/README.md`
