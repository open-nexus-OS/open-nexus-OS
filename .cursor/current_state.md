# Current State — Open Nexus OS

Last updated: 2026-05-29

## Active focus

**RFC-0061: Selftest Observer + nexus-init Module Refactoring**

### M1 — Contract Tests ✅
Per-service contract tests created for keystored (44 tests), statefsd (21), samgrd (14).
Existing tests for logd (37), policyd (25), gpud, fbdevd, windowd, inputd.
All pass with `cargo test -p <service>`.

keystored now has `src/protocol.rs` — public wire-format module (host + OS).

### Bootstrap Scaffold ✅
- `bootstrap/policyd.rs` — extracted policyd helpers
- `bootstrap/spawn.rs` — extracted spawn helpers
- `CtrlChannel` → `pub(crate)` in os_payload.rs
- Compiles: `cargo check -p nexus-init --no-default-features --features os-payload`

### Blocked: Full Module Split (R1-R9)
Cannot complete until duplicate types are unified:
- `os_payload.rs::CtrlChannel` (private, ~18 fields) vs `bootstrap/types.rs::CtrlChannel` (pub(crate), 44 fields)
- `os_payload.rs::BootstrapState` vs `bootstrap/types.rs::BootstrapState`

## Key findings

- The `bootstrap_service_images` function is 1865 lines interleaving 6 distinct phases
- Extraction requires: unify types → extract MMIO → extract wiring → extract routes → extract responder
- All new contract test files are host-compilable — zero QEMU dependency for protocol validation

## Previous (complete)

**TASK-0062 / Unified Capability Architecture**: route_table.rs, bootstrap scaffolding landed.
TASK-0062 diagnostics complete (gpud MMIO fault root cause found).
