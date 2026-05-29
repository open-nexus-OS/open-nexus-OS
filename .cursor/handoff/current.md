# Handoff — Unified Capability Architecture (Phase 1)

Date: 2026-05-29
Session: diagnosis + Phase 1 implementation start

## Diagnosis

Root cause chain for black screen:
1. gpud `create_resource()` crashes on MMIO fault (`cap_query` on VMO)
2. init-lite `cap_transfer(dead_pid, ...)` → kernel `TransferError::InvalidChild`
3. init-lite `fatal_err()` → PANIC
4. windowd/inputd/fbdevd unrouted → route fallback → fbdevd recv permission-denied
5. No framebuffer registration → black screen

## Duplicate structures found

- init-lite routing table (os_payload.rs, 3771 lines): hardcoded manual wiring
- samgrd registry (os_lite.rs, 541 lines): stub, scoped slot numbers, not used by init
- Both exist, neither talks to the other

## Plan (4 phases)

| Phase | Goal | Gate |
|-------|------|------|
| P1 | Graceful Wiring + gpud fix | `fbdevd: flush ok` appears |
| P2 | RouteTable as typed struct | All tests green, routing table <200 lines |
| P3 | samgrd as SSOT | selftest-client resolves via samgrd |
| P4 | Newtypes + hop markers | `just test-all` green |

## Phase 1 tasks

1. os_payload.rs: replace `fatal_err()` with per-service error skip
2. gpud/backend.rs: fix `cap_query` on VMO (use vmo_map_page physical address)
3. route_table.rs: new file with typed RouteTable, ServiceId, CapSlot

## Next step

Implement Phase 1: start with route_table.rs as the foundation,
then apply graceful wiring in os_payload.rs, then fix gpud.
