---
schema: 1
title: nexus-init
role: guide
subsystem: app-platform
sources: [source/init/nexus-init, source/libs/nexus-service-topology, scripts/system-volume-services.txt]
contracts: [ADR-0062, ADR-0060, ADR-0081, ADR-0057, RFC-0093, RFC-0087]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# nexus-init

> **Purpose** — what init does from its first instruction to a supervised fleet, and where each part lives.
> **Owns** — `source/init/nexus-init/`; the declarations it reads live in `source/libs/nexus-service-topology/`
> **Contracts** — [ADR-0062](../adr/0062-boot-stage-fence-and-readiness-barriers.md) (readiness + stage fence), [ADR-0060](../adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md) (system volume), [ADR-0081](../adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md) (bootfs), [ADR-0057](../adr/0057-service-restart-capability-re-resolve.md) (restart), [RFC-0093](../rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md), [RFC-0087](../rfcs/RFC-0087-reliability-failure-model-v1.md) (supervision)
> **Invariants** — `init: up <svc>` only after that service's `@ready`; boot is ordered only by `@ready` and the stage fence, never by yields, resume order or timers; a capability lands in the slot the topology declares; a respawn mints nothing the topology does not declare.
> **Read next** — [boot.md](boot.md), [execd.md](execd.md), [lifecycle.md](lifecycle.md), [../services/samgr.md](../services/samgr.md)

## Role

Init is the parent of every boot service. It spawns them, grants their devices and routes,
answers their route asks, gates their exec/route/capability requests through `policyd`, advances
the boot stages, and reaps and restarts them. Boundary decision:
[ADR-0001](../adr/0001-runtime-roles-and-boundaries.md).

Non-goals: business logic of any service (belongs in `userspace/<crate>`), app spawning
(execd), fake progress (`init: ready` only when bring-up really completed).

## Code layout

| Where | What |
|---|---|
| `src/*.rs` (top level) | Pure, host-tested decisions: `boot_graph.rs` (boot targets → resume sets), `stage.rs` (when a stage may be signalled), `ready_table.rs` (`@ready` bookkeeping), `route_table.rs` + `route_park.rs` (routes, stale marks, parked asks), `service_supervision.rs` + `supervision_engine.rs` (restart policy, backoff, crash-loop cap), `respawn_plan.rs` (what a respawn re-pins), `service_source.rs` (bootfs vs. system volume), `service_topology.rs` (re-export of `nexus-service-topology`), `affinity.rs` (CPU placement). |
| `src/os_payload.rs` | Public types (`ServiceImage`, `InitError`) and thin wrappers. |
| `src/bootstrap/` | The OS orchestration, one module per phase or plane: bootfs and device tree (`bootfs.rs`, `device_tree.rs`, `pci.rs`, `boot_disk.rs`), spawning (`spawn.rs`, `volume_spawn.rs`), planes and grants (`core_plane.rs`, `blk_plane.rs`, `device_planes.rs`, `soc_glue.rs`), wiring (`endpoints.rs`, `distribute.rs`, `declared_slots.rs`, `declared_routes.rs`, `wiring.rs`, `execd_wiring.rs`), resume and stages (`resume.rs`, `stage_signal.rs`, `handshake.rs`), the runtime loop (`responder.rs`, `route_reply.rs`, `policyd.rs`), supervision (`supervision.rs`, `respawn.rs`, `windowd_respawn.rs`, `persist.rs`). `orchestrator.rs` (`run_bootstrap`) sequences them. |

The module split follows [RFC-0061](../rfcs/RFC-0061-selftest-observer-init-refactoring.md);
every file stays under the structure ratchet.

## Boot sequence

1. **Bootfs spawn** — init maps the bootfs table the kernel put in its fixed slot
   (`bootstrap/bootfs.rs`) and `exec_vmo`s each core service from its sealed file object,
   suspended (`init: start <svc>`). execd receives the app-host's and the recv-wake probe's
   objects in its declared slots. Nothing is embedded in init and nothing is copied.
2. **Core plane (wave 0)** — `core_plane.rs`: policyd first and alone; then socd (tree alias +
   provider windows); then the boot-disk grant to blkd (`init: boot disk ok (…)`), blkd and
   bundlemgrd. Only these run yet: a service resumed without its server pair would retry its
   route probe and fill init's control plane.
3. **Volume spawn pass** — `volume_spawn.rs`: for each service in
   `scripts/system-volume-services.txt`, ask bundlemgrd (`QUERY_BUNDLE`, `GET_BUNDLE_IMAGE`),
   receive a sealed, digest-checked image and `exec_vmo` it (`init: spawn from volume svc=…`).
   A failure is honest absence (`init: volume spawn FAIL svc=<n> reason=<r>`).
   `service_source.rs` includes the same list at compile time; a host test keeps CORE services
   (`boot_graph::CORE`) off the volume so recovery boots never depend on it.
4. **Endpoints and server pairs** for all services (bootfs and volume alike), then wave 1.
5. **Device grants, wiring, boot-attempt handshake** with bootctld (`handshake.rs`), then the
   remaining waves and the display drivers per the resolved boot graph
   (`init: stage graph target=<label>`).
6. **Stages** — init owns the one kernel timeline fence per boot and signals
   `platform` → `display-ready` → `session-start` → `shell-visible` (`stage: <name>`) when each
   stage's members announced `@ready`. Children hold a WAIT-only copy.
7. **Responder loop** — `responder.rs`: one waitset over every control channel plus init's timer.
   It serves route asks, policy checks (exec, route, capability), `@ready` announces, the
   update health gate (`init: health ok (slot <a|b>)`), and the supervision sweep.

`init: ready` marks the end of the baseline bring-up; `init: timing …` reports its cost.

## Readiness

- A service is ready when it calls `nexus_service_entry::ready`, which prints `<svc>: ready` and
  sends `@ready` on its control channel. Init records it (`ready_table.rs`) and prints
  `init: up <svc>`. A refused announce is `init: FAIL ready <reason> svc=<svc>`.
- `scripts/check-init-sync.sh` keeps that the only path to `": ready"` and `init: up`.
- Route asks never wait on readiness (that would be cyclic); ordering by readiness is the stage
  fence's job.

## Policy

Before granting a route, a capability or an exec, init asks `policyd` (`bootstrap/policyd.rs`).
Identity is the kernel `sender_service_id`. See
[ADR-0014](../adr/0014-policy-architecture.md) and
[signing.md](../security/signing.md).

## Supervision

Init reaps its children and decides restarts; details in [lifecycle.md](lifecycle.md).
In short: every death is announced (`init: service exit name=<svc> reason=<label> code=0x…`)
and marked stale in the route table; respawnable services restart with backoff from the same
sealed image and declared topology; restart counters persist at `/state/init/restarts/<svc>`.

## Debugging bring-up

1. `RUN_UNTIL_MARKER=1 just test-os`, open `build/logs/latest/uart.log`.
2. Find the last `init:` marker and the last `<svc>: ready`.
3. No `init: start` → userspace never came up; look at kernel/loader.
4. `init: start <svc>` without `init: up <svc>` → the service never announced (crashed, blocked
   or never ran) — look for `init: service exit` / `init: FAIL ready`.
5. Missing `stage:` line → one member of that stage's barrier never announced.

Marker truth: `source/apps/selftest-client/proof-manifest/`. Do not copy marker lists here.
