# Handoff — RFC-0061: Selftest Observer + nexus-init Module Split

Date: 2026-05-29
Session 3: Policyd + Spawn wiring complete

## Progress

### os_payload.rs line count
- Before: 3903 lines
- After: 3523 lines (-380, -9.7%)

### Bootstrap modules (all wired)

| Module | Functions | Lines | Status |
|--------|-----------|-------|--------|
| `bootstrap/types.rs` | CtrlChannel, BootstrapState | 49 | ✅ wired |
| `bootstrap/policyd.rs` | policyd_route_allowed, policyd_cap_allowed, policyd_exec_allowed | 152 | ✅ wired |
| `bootstrap/route_builder.rs` | build_route_table, populate_samgrd_registry | 121 | ✅ wired |
| `bootstrap/spawn.rs` | spawn_service, spawn_service_with_probe | 47 | ✅ wired |
| `bootstrap/mod.rs` | module registry + re-exports | 14 | ✅ |

### Visibility changes in os_payload.rs
- `POLICY_NONCE` → `pub(crate)`
- `debug_write_byte`, `debug_write_bytes`, `debug_write_str`, `debug_write_hex` → `pub(crate)`
- `CtrlChannel` → removed (uses bootstrap/types.rs)
- `BootstrapState` → removed (uses bootstrap/types.rs)

## Remaining in os_payload.rs (3523 lines)

The big remaining chunks:
1. **`bootstrap_service_images`** (~1800 lines) — the service spawn + wiring orchestrator
2. **`service_main_loop_images`** (~300 lines) — the routing responder loop
3. **Helper functions** (~200 lines): `grant_mmio_cap`, `updated_boot_attempt`, `bundlemgrd_set_active_slot`, `updated_health_ok`, `decode_init_health_ok_req` family, `probe_virtio_mmio_slots`, `fatal`, `watchdog_limit_ticks`, etc.
4. **Types/constants** (~200 lines): `ServiceImage`, `InitError`, `ReadyNotifier`, `ServiceNameGuard`, various constants

## Next step

Extract `service_main_loop_images` → `bootstrap/responder.rs`. This requires:
1. Making helper functions `pub(crate)`: `decode_init_health_ok_req`, `encode_init_health_ok_rsp`, `decode_route_get_with_optional_nonce`, `updated_health_ok`, `abi_error_label`, `ipc_error_label`, `watchdog_limit_ticks`
2. Moving the function body (~300 lines) to bootstrap/responder.rs
3. Replacing the os_payload.rs version with a thin wrapper that calls bootstrap

## Files changed

- `source/init/nexus-init/src/bootstrap/policyd.rs` (rewritten with real v3 protocol)
- `source/init/nexus-init/src/bootstrap/mod.rs` (route_builder + policyd registered)
- `source/init/nexus-init/src/os_payload.rs` (-380 lines: types unified, policyd/spawn/route_builder extracted)
