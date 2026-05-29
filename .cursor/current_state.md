# Current State — Open Nexus OS

Last updated: 2026-05-29

## Active focus

**TASK-0062 / Unified Capability Architecture (Phase 1/4)**
Diagnosed: gpud MMIO fault → init-lite fatal crash → service mesh broken → black screen.
Root cause: `cap_query()` on VMO fails; init-lite has no graceful degradation.

Plan: 4-phase capability architecture overhaul (Fuchsia/OHOS-inspired).
- Phase 1: Graceful Wiring + gpud fix (IN PROGRESS)
- Phase 2: RouteTable as typed data structure
- Phase 3: samgrd as single source of truth
- Phase 4: Newtypes, error propagation, hop markers

## Architecture (capability)

```
Current (broken):                  Target (Phase 3):
┌──────────────────────┐          ┌──────────────────────┐
│ os_payload.rs (3771) │          │ init-lite (Component  │
│ • manual cap_transfer│          │   Manager light)     │
│ • hardcoded routing  │          │ • RouteTable         │
│ • fatal on error     │          │ • graceful degr.     │
└──────────────────────┘          └──────┬───────────────┘
                                        │ populate
┌──────────────────────┐          ┌──────┴───────────────┐
│ samgrd (stub)        │          │ samgrd (SSOT)         │
│ • slot numbers only  │          │ • endpoint caps       │
│ • hardcoded allowlist│          │ • global registry     │
│ • not used by init   │          │ • health/restart      │
└──────────────────────┘          └──────────────────────┘
```

## Key findings (2026-05-29 session)

- gpud: probe() succeeds, create_resource() fails — `cap_query()` on vmo_create() VMO
- init-lite: `cap_transfer` to dead PID → kernel returns TransferError::InvalidChild → init fatals
- samgrd: exists but is stub; duplicates init-lite routing; not used for service mesh
- 3771-line os_payload.rs: monolithic, hardcoded, no error isolation

## Previous (complete)

**TASK-0062 / RFC-0059: Implemented (Phases 0-5 complete).**
38 tests green. Animation Engine + NexusGfx SDK + GfxBackend + gpud driver.
Implicit transitions integrated. RISC-V optimizations applied.
