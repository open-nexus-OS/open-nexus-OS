---
schema: 1
title: "Service manager (samgrd + samgr)"
role: guide
subsystem: app-platform
sources: [source/services/samgrd, userspace/samgr, source/init/nexus-init/src/route_table.rs, source/init/nexus-init/src/bootstrap/route_builder.rs, source/libs/nexus-wire/src/routing.rs]
contracts: [ADR-0017, ADR-0057, RFC-0005, RFC-0093]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Service manager (samgrd + samgr)

> **Purpose** — who resolves a service name to capabilities, what samgrd does, and what the host `samgr` crate is for.
> **Owns** — `source/services/samgrd/`, `userspace/samgr/`; the resolve authority is init's `RouteTable` + responder
> **Contracts** — [ADR-0017](../adr/0017-service-architecture.md), [ADR-0057](../adr/0057-service-restart-capability-re-resolve.md) (restart = re-resolve), [RFC-0005](../rfcs/RFC-0005-kernel-ipc-capability-model.md), [RFC-0093](../rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md) (§1 route asks)
> **Invariants** — registrations are scoped to the kernel `sender_service_id`; `RESOLVE_STATUS` never returns capability slots; the OS build never links the in-process registry.
> **Read next** — [README.md](README.md), [../boot-init/nexus-init.md](../boot-init/nexus-init.md), [../boot-init/lifecycle.md](../boot-init/lifecycle.md)

## Who resolves what

| Question | Answered by |
|---|---|
| "Give me a route to service X" | **init's responder** (`OP_ROUTE_GET`, `source/libs/nexus-wire/src/routing.rs`) from its `RouteTable`, policy-checked via policyd |
| "Is X registered?" (status only) | **samgrd** `OP_RESOLVE_STATUS` |
| Host tests: register / resolve / restart / heartbeat | **`userspace/samgr`** in-memory registry |

Most routes are pinned before a service runs (declared slots); a runtime route ask goes to init.
samgrd is a secondary registry. ADR-0057 records this: staleness lives where resolving happens
(init); samgrd would inherit the same contract if it ever became the broker.

## Restart and staleness (ADR-0057)

- A dead service is marked stale in init's `RouteTable`; `OP_ROUTE_GET` for it is parked and
  answered once it is re-provisioned (overflow: `STATUS_STALE`, distinct from not-found).
- Clients re-resolve on `PeerClosed`/EOF or a stale answer. No kernel-side capability rewriting.
- Details: [../boot-init/lifecycle.md](../boot-init/lifecycle.md).

## samgrd (OS)

- Boot service, spawned by init; announces `samgrd: ready` at start, no route asks at start-up.
- Init fills it at boot: `populate_samgrd_registry` (`source/init/nexus-init/src/bootstrap/route_builder.rs`)
  sends `OP_REGISTER` for each core service's route.
- Registry key = `(sender_service_id, name)`: one service cannot publish entries another
  service's `OP_LOOKUP` sees.
- Clients: `selftest-client` (register/lookup/attribution probes), dsoftbusd's remote proxy
  (forwards a remote peer's `RESOLVE_STATUS` frame).

Wire `[S, M, 1, op, …]` (`source/services/samgrd/src/os_lite.rs`):

| Op | Name | Request → response |
|---|---|---|
| 1 | `OP_REGISTER` | `name_len, send:u32, recv:u32, name` → status |
| 2 | `OP_LOOKUP` | `name_len, name` → status, send, recv (caller's own scope) |
| 3 | `OP_PING_CAP_MOVE` | optional nonce → `PONG` on the moved reply cap |
| 4 | `OP_SENDER_PID` | nonce → kernel-observed sender pid |
| 5 | `OP_SENDER_SERVICE_ID` | nonce → kernel `sender_service_id` |
| 6 | `OP_RESOLVE_STATUS` | `name_len, name` (≤ `MAX_SERVICE_NAME_LEN`) → status only, across all scopes |
| 0x7f | `OP_LOG_PROBE` | → appends a structured record to logd (core-log proof) |

Status: `0` ok, `1` not found, `2` malformed, `3` unsupported. Replies ride the moved reply
capability (`reply_recv`), so a served request costs one kernel entry.

Proof: `samgrd: ready`, the `SELFTEST: samgrd …` probes and the core-log checks in
`source/apps/selftest-client/proof-manifest/markers/`. Host contract tests:
`source/services/samgrd/tests/`.

## samgrd (host backend)

`src/std_server.rs` serves the Cap'n Proto `samgr.capnp` schema
(`tools/nexus-idl/schemas/samgr.capnp`) over `userspace/samgr::Registry` — used by host E2E
tests.

## `userspace/samgr` (host library)

In-memory registry for host tests (`nexus_env = "host"`):

| Call | Effect |
|---|---|
| `register(name, endpoint)` | new entry; `Duplicate` if the name exists |
| `resolve(name)` | latest endpoint + generation; `NotFound` |
| `heartbeat(handle)` | refreshes liveness; `StaleHandle` for an old generation |
| `restart(name, endpoint)` | bumps the generation, replaces the endpoint |

Generations are monotonic `u64`, so a caller can reject a handle from before a restart. The
`nexus_env = "os"` backend returns `Unsupported` for every call: no in-process registry in OS
builds. The crate uses `parking_lot`, which is why it must stay out of the OS graph
([RFC-0009 dep gate](../rfcs/RFC-0009-no-std-dependency-hygiene-v1.md)).
