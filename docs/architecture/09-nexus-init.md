# `nexus-init` (boot orchestration) — onboarding

`nexus-init` is the **orchestrator** for userspace bring-up.
It is the place where "what the OS boots" becomes concrete and testable.

This page is intentionally high-level and drift-resistant:

- **Canonical QEMU smoke contract** (marker ordering): `scripts/qemu-test.sh`
- **Contributor workflow + marker guidance**: `docs/testing/README.md`
- **Role/boundary decision**: `docs/adr/0001-runtime-roles-and-boundaries.md`
- **Module split (RFC-0061)**: `docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md`

## Module structure (post RFC-0061)

`nexus-init` was refactored from a monolithic `os_payload.rs` (3903 lines)
into 8 focused `bootstrap/` modules (3540 lines) plus a thin `os_payload.rs`
(404 lines — public types and thin wrappers).

```
source/init/nexus-init/src/
├── os_payload.rs          ← public types (ServiceImage, InitError) + thin wrappers
├── route_table.rs         ← RouteTable, ServiceId, CapSlot
├── bootstrap/
│   ├── types.rs           ← CtrlChannel, BootstrapState
│   ├── spawn.rs           ← spawn_service_with_probe
│   ├── policyd.rs         ← policyd_route/cap/exec_allowed (v3 protocol)
│   ├── route_builder.rs   ← build_route_table, populate_samgrd_registry
│   ├── responder.rs       ← run_responder_loop (route-get, health-ok, exec-check)
│   ├── helpers.rs         ← MMIO probing, OTA, health checks, debug helpers
│   └── orchestrator.rs    ← run_bootstrap (spawn + endpoints + wiring)
```

## Responsibilities (what `nexus-init` owns)

- **Bring-up sequencing**: starting the required daemons in a deterministic order.
- **Readiness observation**: emitting `init: up <svc>` only after the service is actually up.
- **Policy gating**: consulting `policyd` before allowing a service to launch with requested capabilities (see `docs/security/signing-and-policy.md`).
- **Service graph glue**: ensuring core authorities exist early (e.g. `policyd`, `samgrd`, `bundlemgrd`, `packagefsd`, `vfsd`, `execd`, …).
- **Update health gate (v1.0)**: issue `updated.BootAttempt()` at boot, forward health commits, and emit `init: health ok (slot <a|b>)` (see `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`).

## Non-goals (what it should not accumulate)

- Business logic for any service (belongs in `userspace/<crate>`).
- Duplicated loader logic (belongs in `userspace/nexus-loader` and `execd`).
- “Fake green” progress reporting (no `init: ready` if the system is not actually ready).

## Markers and ownership (why `init:` lines exist)

The system uses deterministic markers so QEMU runs are a real proof:

- `init: start` is the *start of orchestration*.
- `init: start <svc>` announces that init requested a service spawn.
- `init: up <svc>` is init’s **observation** that `<svc>` reached readiness.
- `init: ready` means the baseline bring-up sequence completed.

Each service also emits `<svc>: ready` once it can accept requests.

**Important:** The definitive list and ordering is enforced in `scripts/qemu-test.sh`. Do not copy the full list into docs; link to the harness.

## Where policy fits

At boot, `nexus-init` queries `policyd` for each service before launching it.
Denials must be explicit and are part of the proof story (host E2E + QEMU markers).

See:

- `docs/security/signing-and-policy.md`
- `docs/adr/0014-policy-architecture.md`

## How to debug bring-up failures

Start from the canonical harness outputs:

- `uart.log` (markers + service logs)
- `qemu.log` (QEMU diagnostics)

Workflow:

1. Run `RUN_UNTIL_MARKER=1 just test-os` (or `./scripts/qemu-test.sh`) and inspect `uart.log`.
2. Find the last `init:` marker and the last `<svc>: ready`.
3. If `init: start` is missing, userspace didn’t come up; focus on kernel/loader/early boot.
4. If `init: start <svc>` appears but `init: up <svc>` does not, the service likely failed to reach readiness.
