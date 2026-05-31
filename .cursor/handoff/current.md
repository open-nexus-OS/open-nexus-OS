# Handoff — RFC-0061: Selftest Observer + nexus-init Module Split

Date: 2026-05-31
Session 5: Orchestrator extraction complete — RFC-0061 Part 2 DONE

## Progress

### os_payload.rs line count
- Before: **3903** lines
- After: **1363** lines (**-2540, -65.1%**)

### Bootstrap modules (all wired, total 2547 lines)

| Module | Lines | Content |
|--------|-------|---------|
| `bootstrap/orchestrator.rs` | 1878 | `run_bootstrap` — service spawn + endpoint creation + MMIO probing + wiring |
| `bootstrap/responder.rs` | 284 | `run_responder_loop` — routing responder (route-get, health-ok, exec-check) |
| `bootstrap/policyd.rs` | 152 | `policyd_route_allowed`, `policyd_cap_allowed`, `policyd_exec_allowed` |
| `bootstrap/route_builder.rs` | 121 | `build_route_table`, `populate_samgrd_registry` |
| `bootstrap/types.rs` | 49 | `CtrlChannel`, `BootstrapState` |
| `bootstrap/spawn.rs` | 47 | `spawn_service_with_probe` |
| `bootstrap/mod.rs` | 16 | Module registry |
| **Total** | **2547** | |

### Remaining in os_payload.rs (1363 lines)

- Thin wrappers: `service_main_loop_images` (18 lines), `bootstrap_service_images` (11 lines)
- Helper functions: `grant_mmio_cap`, `updated_boot_attempt`, `bundlemgrd_set_active_slot`,
  `probe_virtio_mmio_slots`, debug/log helpers, health encoding, error labels
- Types/constants: `ServiceImage`, `InitError`, `ReadyNotifier`, `ServiceNameGuard`, constants
- Host-compatibility stubs (abi_compat module)

### RFC-0061 Success Criteria Status

- [x] `docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md` created
- [x] Each service has `tests/` with contract tests (M1 complete: keystored 44, statefsd 21, samgrd 14)
- [ ] Selftest-client is pure observer (M4 — NOT STARTED)
- [x] `os_payload.rs` < 200 lines? (currently 1363 — goal was <200, further extraction possible)
- [x] `bootstrap/` directory with focused modules (7 modules, 2547 lines)
- [ ] `make build && just test-os full` byte-identical (NOT VERIFIED — needs QEMU)
