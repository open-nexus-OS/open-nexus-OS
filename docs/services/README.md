---
schema: 1
title: "Services guide"
role: guide
subsystem: app-platform
sources: [source/services, source/libs/nexus-service-entry, source/libs/nexus-service-topology, source/libs/nexus-wire, config/service-layout.allow]
contracts: [ADR-0017, ADR-0051, RFC-0005, RFC-0040, RFC-0003]
verified: 2026-11-12
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Services guide

> **Purpose** — how a userspace service is structured, wired, proven and observed.
> **Owns** — the conventions for `source/services/<name>d/`; entry, topology and wire crates in `source/libs/`
> **Contracts** — [ADR-0017](../adr/0017-service-architecture.md) (service architecture), [ADR-0051](../adr/0051-declarative-wire-codec-nexus-wire.md) (wire codec), [RFC-0005](../rfcs/RFC-0005-kernel-ipc-capability-model.md) (IPC + capabilities), [RFC-0040](../rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md) (VMO bulk), [RFC-0003](../rfcs/RFC-0003-unified-logging.md) (logging)
> **Invariants** — a service is a thin adapter: domain logic lives in a host-testable crate; `<svc>: ready` only when it can serve; identity is the kernel `sender_service_id`; every input bounded before parsing.
> **Read next** — [samgr.md](samgr.md), [bundlemgrd.md](bundlemgrd.md), [../boot-init/nexus-init.md](../boot-init/nexus-init.md), [../boot-init/lifecycle.md](../boot-init/lifecycle.md)

## What a service is

A process under `source/services/<name>d/` (drivers under `source/drivers/`) that:

- is spawned and wired by init from the declared topology;
- serves a small byte-frame protocol over kernel IPC;
- validates and bounds every request, answers with stable status codes;
- forwards into a domain crate in `userspace/` (or a `source/libs/` core crate).

If a service accumulates real logic, the boundary is leaking: move it into the crate.

## Layout

| Part | Rule |
|---|---|
| `src/lib.rs` | `#![forbid(unsafe_code)]`, CONTEXT header; typically an `os_lite` module (`nexus_env = "os"` + `os-lite`) and a `std_server` host backend |
| `src/main.rs` | OS entry via `nexus_service_entry::declare_entry!`, host `main` |
| `tests/` | contract and `test_reject_*` tests; new services must ship it (`config/service-layout.allow` lists the grandfathered ones) |
| Build | OS: `--no-default-features --features os-lite`; host: default features |
| Size | ≤ ~600 LOC per file (structure ratchet) |

## Wiring

- **Declarations**: `source/libs/nexus-service-topology/` holds service ids, routes and the
  capability slots each service gets. Init pins every capability into the slot declared there;
  the service reads the same constants (`nexus_service_topology::slots::<svc>`). A service that
  stands on a device plane a machine may lack declares it there too (`planes.rs`,
  `PLANE_NEEDS`): init resumes it only where the device tree names that plane.
- **Readiness**: `nexus_service_entry::ready("<svc>: ready")` prints the marker and sends
  `@ready` to init; nothing else may print `": ready"`.
- **Routes**: a service asks init's responder for a route to another service; see
  [samgr.md](samgr.md) for who resolves what.

## Control plane and data plane

| Plane | Mechanism | Rules |
|---|---|---|
| Control | Versioned byte frames `[magic, magic, ver, op, …]`; codecs in `source/libs/nexus-wire/` (ADR-0051); host backends may use Cap'n Proto schemas in `tools/nexus-idl/schemas/` | small (fits one IPC frame), bounded, nonce-correlated where replies can interleave |
| Data | VMO: producer arms a VMO capability, writes, header last, replies; consumer maps or reads it | bulk never goes inline; small payloads may |

VMO users today:

| Path | Ops |
|---|---|
| Service images and app payloads | bundlemgrd `GET_BUNDLE_IMAGE` (sealed), `GET_PAYLOAD` |
| File content | vfsd `OP_READ_VMO` → packagefsd `READ_VMO` → bundlemgrd `GET_FILE_VMO` |
| Block reads | blkd `OP_READ_VMO` ([ADR-0044](../adr/0044-single-blk-device-gpt-partitions-block-layer.md)) |

VMO semantics and proofs: [../storage/vmo.md](../storage/vmo.md).

## Observability

- Log through the `nexus-log` facade (`source/libs/nexus-log/`) into logd
  (`source/services/logd/`, bounded journal: APPEND / QUERY / STATS). No ad-hoc prints.
- UART markers are the proof contract; logs are not markers.
- Crash reports come from the parent: execd for apps ([../boot-init/execd.md](../boot-init/execd.md)),
  init for boot services.
- Never log secrets, keys or keystrokes.

## Tests

| Layer | Where |
|---|---|
| Behavior | unit/property tests in the domain crate |
| Wire contract + rejects | `source/services/<name>d/tests/` |
| Integration | host E2E under `tests/` |
| OS proof | marker ladder: `source/apps/selftest-client/proof-manifest/`, `just test-os` |

`selftest-client` only observes; each service owns its gates. Methodology:
[../testing/README.md](../testing/README.md).
