# Architecture index (`docs/architecture/`)

This directory contains **high-level architecture notes** for key subsystems.
These pages are intended to be stable entrypoints; avoid duplicating fast-moving contracts (like full UART marker sequences) and instead link to the canonical sources.

## Suggested reading order (onboarding)

1. `05-system-map-and-boundaries.md` — repo mental model + hard boundaries
2. `06-boot-and-bringup.md` — boot chain + who owns which markers
3. `07-contracts-map.md` — where contracts live (Tasks/RFC/ADR) + key canonical specs
4. `08-service-architecture-onboarding.md` — services as thin adapters over userspace libraries
5. Service landings (pick what you’re touching):
   - `09-nexus-init.md`
   - `10-execd-and-loader.md`
   - `11-policyd-and-policy-flow.md`
   - `12-storage-vfs-packagefs.md`
   - `13-identity-and-keystore.md`
   - `14-samgrd-service-manager.md`
   - `15-bundlemgrd.md`

## Start here

- **Kernel + layering quick reference (canonical entry page)**: `docs/ARCHITECTURE.md`
- **Testing methodology (host-first, QEMU-last)**: `docs/testing/index.md`
- **Execution truth / workflow**: `tasks/README.md`
- **RFC process / contracts vs tasks**: `docs/rfcs/README.md`

## Kernel

- `01-neuron-kernel.md` — NEURON kernel overview (syscalls, memory model, invariants)
- `hardening-status.md` — kernel hardening objectives checklist (status snapshot)

## Testing + CI

- `02-selftest-and-ci.md` — how we validate: deterministic markers + CI wiring (high level)
- **Canonical QEMU harness + marker contract**: `scripts/qemu-test.sh` (do not duplicate marker lists here)
- **CI workflows**: `.github/workflows/ci.yml`, `.github/workflows/build.yml`

## Networking

- `networking-authority.md` — Canonical vs alternative networking paths, anti-drift rules
- **RFC-0006**: Userspace Networking v1 (sockets facade)
- **RFC-0007**: DSoftBus OS Transport v1 (UDP discovery + TCP sessions)
- **RFC-0008**: DSoftBus Noise XK v1 (handshake + identity binding)
- **RFC-0009**: no_std Dependency Hygiene v1 (OS build policy)

**Current Status (2026-01-07)**:
- TASK-0003/3B/3C: ✅ Done (loopback scope)
- TASK-0004: Next (dual-node + identity binding)

## Services and contracts

### Domain libraries (host-first)

- `03-samgr.md` — `userspace/samgr` (host-first registry library; OS uses `samgrd`)
- `04-bundlemgr-manifest.md` — `userspace/bundlemgr` TOML manifest parser (developer/tests; OS packaging contract is `manifest.nxb`)

### OS daemons (authorities)

- `14-samgrd-service-manager.md` — `samgrd` (OS service registry authority)
- `15-bundlemgrd.md` — `bundlemgrd` (OS bundle/package authority)

## Onboarding landing pages (this directory)

- `05-system-map-and-boundaries.md`
- `06-boot-and-bringup.md`
- `07-contracts-map.md`
- `08-service-architecture-onboarding.md`
- `09-nexus-init.md`
- `10-execd-and-loader.md`
- `11-policyd-and-policy-flow.md`
- `12-storage-vfs-packagefs.md`
- `13-identity-and-keystore.md`
- `14-samgrd-service-manager.md`
- `15-bundlemgrd.md`
