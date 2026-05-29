# RFC-0061: Selftest Observer + nexus-init Module Refactoring

**Status:** In Progress
**Created:** 2026-05-29
**Author:** @jenning
**Depends on:** RFC-0059 (UI v5a Animation + NexusGfx SDK), TASK-0062 (Capability Architecture)

## Summary

Two structural refactorings with zero functional change:

1. **Selftest-Client → Pure Observer**: Active test logic moves from selftest-client into per-service `tests/` contract tests. Selftest-client becomes a marker-reader only.

2. **nexus-init Module Split**: 3900-line `os_payload.rs` monolith broken into 9 focused modules under `bootstrap/`, each with a single responsibility.

## Motivation

### Why split os_payload.rs

- **3873 lines** in a single file — impossible to navigate, review, or test in isolation
- **44 slot fields** on `CtrlChannel` — adding a new service requires touching 6 locations
- **6 distinct jobs** in one function: spawn, endpoints, policyd, MMIO, wiring, routing
- **No unit-testable boundaries** — every phase depends on QEMU for validation

### Why convert selftest-client to observer

- Selftest-client currently IS the test harness, not an observer — it actively calls service IPC
- Adding a new service test requires modifying a centralized test client
- Cannot run `cargo test -p keystored` independently — all tests are coupled
- Per-service contract tests are the OHOS/Fuchsia gold standard

## Architecture: Per-Service Contract Tests

``` text
source/services/keystored/tests/
├── crud_contract.rs        put/get/del → expected results
└── auth_contract.rs        wrong auth → STATUS_DENIED

source/services/statefsd/tests/
├── persist_contract.rs     put → restart → get → matches
└── unauthorized_contract.rs

source/services/samgrd/tests/
├── register_contract.rs    OP_REGISTER → OP_LOOKUP → match
└── malformed_contract.rs   bad frame → STATUS_MALFORMED

source/services/windowd/tests/
├── compose_contract.rs     send frame → compose_hz > 0
└── present_contract.rs     damage rect → present_hz > 0

source/services/inputd/tests/
├── pointer_contract.rs     inject pointer → delivered
└── focus_contract.rs       focus target → routed correctly

source/services/policyd/tests/
├── allow_contract.rs       known cap → STATUS_ALLOW
└── deny_contract.rs        unknown cap → STATUS_DENY

source/services/logd/tests/
├── append_contract.rs      append → query returns it
└── paging_contract.rs      overflow → paging works

source/drivers/gpud/tests/
├── probe_contract.rs       virtio-gpu → mmio probe ok
├── resource_contract.rs    create_resource → MMIO works
└── scanout_contract.rs     set_scanout → display active

source/services/fbdevd/tests/
├── ramfb_contract.rs       fw_cfg → ramfb configured
└── framebuffer_contract.rs VMO alloc → writable
```

Each test runs as `cargo test -p <service>` and emits UART markers
identical to the current selftest-client.

### Observer (selftest-client after refactoring)

``` text
selftest-client/src/observer/
├── mod.rs              dispatcher: phase loop
├── markers.rs          reads UART markers from logd
├── telemetry.rs        polls windowd/fbdevd telemetry
└── liveness.rs         polls samgrd for service health

Reads only. Never initiates service IPC.
```

## Architecture: nexus-init Module Split

``` text
nexus-init/src/
├── lib.rs                         ← public API, re-exports
├── error.rs                       ← InitError (unchanged)
├── types.rs                       ← ServiceImage, CtrlChannel, BootstrapState
├── route_table.rs                 ← RouteTable, ServiceId, CapSlot (from TASK-0062)
├── os.rs                          ← main entry, calls bootstrap + responder
│
├── bootstrap/                     ← bootstrap_service_images, split by phase
│   ├── mod.rs                     ← orchestrator (~50 lines)
│   ├── spawn.rs                   ← spawn loop + exec_v2 (~100)
│   ├── endpoints.rs               ← 40 ipc_endpoint_create_for calls (~200)
│   ├── policyd.rs                 ← pol_ctl_route_req/rsp, slot pinning (~80)
│   ├── priority_wire.rs           ← early wiring for policyd + display (~80)
│   ├── mmio_grants.rs             ← probe + grant_mmio_cap (~200)
│   ├── wire.rs                    ← main wiring loop, 22 focused functions (~900)
│   ├── route_builder.rs           ← build_route_table + populate_samgrd (~150)
│   └── responder.rs               ← routing responder main loop (~200)
```

### Orchestrator (mod.rs)

```rust
pub fn bootstrap_service_images(images, notifier) -> Result<BootstrapState> {
    let mut ctrl_channels = spawn::spawn_all(images)?;
    let pids = ServicePids::from_channels(&ctrl_channels)?;
    let endpoints = endpoints::create_all(&pids)?;
    let pol_ctl = policyd::setup_control(pids.policyd)?;
    priority_wire::wire_display_services(&mut ctrl_channels, &endpoints)?;
    let mmio = mmio_grants::grant_all(&pids, &pol_ctl, &endpoints)?;
    let _ = nexus_abi::yield_();
    wire::wire_all(&mut ctrl_channels, &endpoints, &mmio)?;
    let route_table = route_builder::build(&ctrl_channels);
    route_builder::populate_samgrd(&route_table);
    Ok(BootstrapState { ctrl_channels, route_table, ... })
}
```

## Migration Strategy

### Part 1 — Observer (4 phases)

| # | What | Gate |
|---|------|------|
| M1 | Create `tests/` dirs + contract test skeletons | `cargo test -p keystored` compiles |
| M2 | Move bringup.rs/keystored → services/keystored/tests/ | QEMU markers identical |
| M3 | Move remaining phases one at a time | Each: QEMU marker ladder |
| M4 | Convert selftest-client to pure observer | QEMU byte-identical |

### Part 2 — Module Split (9 phases)

| # | What | Lines | Gate |
|---|------|-------|------|
| R1 | types.rs | +50, -150 | make build |
| R2 | spawn.rs | +100, -80 | just test-os full |
| R3 | endpoints.rs | +200, -180 | just test-os full |
| R4 | policyd.rs | +80, -70 | just test-os full |
| R5 | mmio_grants.rs | +200, -180 | just test-os full |
| R6 | wire.rs (largest) | +900, -850 | just test-os full |
| R7 | priority_wire.rs | +80, -70 | just test-os full |
| R8 | route_builder.rs | +150, -130 | just test-os full |
| R9 | responder.rs | +200, -180 | just test-os full |

## Design Principles

1. **No functional changes** — byte-identical UART output, identical QEMU markers
2. **Extract first, optimize later** — CtrlChannel stays as-is
3. **One module = one purpose** — spawn, wire, grant, respond
4. **Public API stays stable** — `service_main_loop_images` unchanged
5. **No circular dependencies** — DAG: types → spawn → endpoints → policyd → priority_wire → mmio → wire → route_builder → responder

## Success Criteria

- [x] `docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md` created
- [ ] Each service has `tests/` with contract tests (M1-M3)
- [ ] Selftest-client is pure observer (M4)
- [ ] `os_payload.rs` < 200 lines (R1-R9)
- [ ] `bootstrap/` directory with 9 focused modules (R1-R9)
- [ ] `make build && just test-os full` byte-identical (every R phase)
