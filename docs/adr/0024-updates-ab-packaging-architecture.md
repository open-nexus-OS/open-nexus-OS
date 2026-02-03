# ADR-0024: Updates A/B Packaging Architecture

Status: Accepted
Date: 2026-02-02
Owners: @runtime

## Context
Open Nexus OS needs a minimal, deterministic A/B update flow in OS-lite. The `updated` service
coordinates staging, switching, and boot attempts, and must persist boot control state so it
survives restarts. Persistence is provided via the `/state` namespace and the `statefsd` service.

## Decision
Define `updated` as the authority for boot control and update orchestration:

- `updated` owns boot control state under `/state/boot/bootctl.v1`.
- The service exposes a compact IPC protocol for stage/switch/health/boot-attempt operations.
- Persistence uses the `statefs` client and is explicitly gated by policy.

## Rationale
- Centralizing boot control avoids split-brain between services.
- `/state` provides a single persistence substrate with explicit durability boundaries.
- A compact IPC protocol supports deterministic selftests and bounded parsing.

## Consequences
- `updated` is a critical authority service; failures affect boot behavior.
- State must be migrated and read on startup before emitting readiness.
- Tests must prove persistence across a restart cycle (soft reboot).

## Invariants
- No secrets or private keys are logged.
- Policy enforcement is deny-by-default for `/state` access.
- All IPC parsing is bounded and deterministic.
- Readiness markers only emit after required initialization is complete.

## Implementation Plan
1. Define IPC frame format and handler logic in `userspace/updates`.
2. Implement OS-lite backend in `source/services/updated/`.
3. Persist boot control state via `statefs` on every mutation + explicit sync.
4. Add selftests proving boot control persistence across a restart cycle.

## References
- `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md`
- `source/services/updated/`
- `userspace/updates/`
- `tests/updates_host/`
