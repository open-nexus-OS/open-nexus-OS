---
schema: 1
title: "Process lifecycle and supervision"
role: guide
subsystem: app-platform
sources: [source/init/nexus-init/src/bootstrap/supervision.rs, source/init/nexus-init/src/service_supervision.rs, source/init/nexus-init/src/supervision_engine.rs, source/init/nexus-init/src/bootstrap/respawn.rs, source/init/nexus-init/src/bootstrap/windowd_respawn.rs, source/services/execd/src/os_lite]
contracts: [ADR-0073, ADR-0056, ADR-0057, ADR-0062, RFC-0013, RFC-0112]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Process lifecycle and supervision

> **Purpose** — how a process becomes ready, dies, is reaped and (for boot services) comes back.
> **Owns** — supervision in `source/init/nexus-init/`; app reaping in `source/services/execd/src/os_lite/`
> **Contracts** — [ADR-0073](../adr/0073-an-object-dies-with-its-last-capability-a-death-is-an-event-on-an-endpoint.md) (death is an event), [ADR-0056](../adr/0056-task-exit-reason-kernel-abi.md) (exit reasons), [ADR-0057](../adr/0057-service-restart-capability-re-resolve.md) (restart = re-resolve), [ADR-0062](../adr/0062-boot-stage-fence-and-readiness-barriers.md) (readiness + stage fence), [RFC-0013](../rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md) (up vs ready), [RFC-0112](../rfcs/RFC-0112-a-services-death-is-a-re-resolve-for-its-clients-the-compositors-restart.md) (compositor restart)
> **Invariants** — only the parent reaps (init for boot services, execd for apps); restart decisions use the kernel exit reason, never the code alone; a restarted instance holds no right the topology does not declare.
> **Read next** — [execd.md](execd.md), [nexus-init.md](nexus-init.md), [RFC-0087](../rfcs/RFC-0087-reliability-failure-model-v1.md), [RFC-0111](../rfcs/RFC-0111-process-lifecycle-freeze-thaw-kill-the-parent-orders-the-process.md)

## Who is whose parent

| Process | Parent | Reaper | Restarts |
|---|---|---|---|
| Boot services (bootfs + system volume) | init | init's supervision sweep | init, per policy table |
| App-hosts, probe children | execd | execd's reaper | never by execd |
| Threads (`spawn`, own address space only) | the spawning task | the spawning task | — |

`wait` is parent-bound: no other process can reap a child.

## Ready

- **Up vs ready** (RFC-0013): `<svc>: ready` is the service's own claim that it can serve;
  `init: up <svc>` is init's observation of that claim. Both come from one call,
  `nexus_service_entry::ready`, which also sends `@ready` on the control channel. `init: up`
  never precedes `<svc>: ready`.
- **Stages** (ADR-0062): one kernel timeline fence per boot, advanced only by init, only when the
  stage's members announced `@ready`: `platform` → `display-ready` → `session-start` →
  `shell-visible`. Never yields, resume order or timers.
- A restarted instance announces afresh: the exit hook clears its ready entry
  (`src/ready_table.rs`).

## Death (kernel)

- A task exits voluntarily (`exit`), faults, or is killed. The kernel attributes the reason
  (ADR-0056): `clean`, `error`, `fault` (with cause), `killed`.
- The task stays a zombie until its parent reaps it with `wait` / `wait_nohang`
  (`wait_*_with_reason` returns the reason). RFC-0081: non-blocking reaping.
- ADR-0073: every kernel object dies with its last capability, and a death is an event — a parent
  that bound an endpoint with `child_exit_bind` (syscall 62) gets one `OP_CHILD_EXITED` frame per
  dead child. execd reaps on that wake. Init sees a child's death through the EOF on that child's
  control endpoint in its responder waitset.

## Supervision (init)

1. **Sweep** — each responder round drains `wait_nohang_with_reason` and announces every death
   once: `init: service exit name=<svc> reason=<label> code=0x<hex>`
   (`bootstrap/supervision.rs`).
2. **Stale mark** — the dead service is marked stale in init's `RouteTable`.
3. **Policy** — `src/service_supervision.rs` gives every boot service a criticality
   (`CriticalBoot`, `CriticalSession`, `Standard`) and a restart policy (`Never`, `OnFailure`,
   `Always`). A `clean` exit never counts toward a crash loop.
4. **Backoff** — one declared schedule (`DEFAULT_BACKOFF`): 500 ms, doubling, capped at 15 s;
   5 failures in 60 s park the service: `init: crash-loop blocked svc=<svc> reason=<r>`.
   Decided by the pure engine `src/supervision_engine.rs`.
5. **Respawn** — `bootstrap/respawn.rs` restarts the services `respawnable()` lists (`pinched`,
   `windowd`). Any other dead service is announced, marked stale, and stays down.
6. **Persist** — before resuming a respawned instance init bumps
   `/state/init/restarts/<svc>` in statefs (`bootstrap/persist.rs`; init holds
   `statefs.read`/`statefs.write` in `policies/base.toml`).
7. **Done** — `init: service restarted name=<svc> pid=0x…` and the stale mark is cleared; a
   failure is `init: FAIL service respawn name=<svc>`.

The proof probes run every boot: `demo.exit0` proves the sweep
(`SELFTEST: init supervision sweep ok`), a fault injector proves restart and cap
(`SELFTEST: supervision restart ok`, `SELFTEST: crash-loop cap ok`).

## Restart and re-resolve (ADR-0057)

- **Restart is derivation**: the new instance runs from the same sealed image (bootfs object or
  the kept volume image) and is provisioned only from the declared topology. Only what died with
  the old instance (its owned endpoints) is re-minted.
- **The resolve authority is init's responder**, not samgrd. A route ask to a stale target is
  parked (`src/route_park.rs`, at most 32 outstanding) and answered once when the target is
  re-provisioned; overflow answers `STATUS_STALE`.
- **Clients re-resolve** on `PeerClosed`/EOF ([RFC-0079](../rfcs/RFC-0079-ipc-last-sender-eof.md))
  or a stale answer, under bounded retry.
- Init's persistent control-plane slots are never marked stale.

## Compositor restart (RFC-0112)

The windowd respawn (`bootstrap/windowd_respawn.rs`, `src/respawn_plan.rs`):

1. Re-execute the sealed image; mint what died with the old instance (request/response
   endpoints, reply inbox, timer, watch channels); pin windowd's declared legs.
2. Drop answers still queued for the dead instance.
3. Re-pin every client leg into windowd over the dead capabilities (`REPLACE_DEAD`); clients
   are read from the declarations (`SERVICE_SPECS`), never from a separate list.
4. Place it on cpu0, resume it, tell execd (`OP_COMPOSITOR_BACK`).

Then:

- windowd tells abilitymgr it is up (`abilitymgr: compositor up (thawed=N)`): frozen apps are
  thawed into the background.
- gpud takes the new instance's first framebuffer request as the end of the old one and releases
  every app surface (`gpud: compositor restarted (owners=N …)`); surfaces are gpud-allocated per
  owner ([ADR-0092](../adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md)).
- execd re-pins each live app's windowd slots and sends `OP_COMPOSITOR_BACK` on the app's event
  channel with a fresh SEND; a full channel makes the notice owed, never a wait
  (`execd: compositor back (apps=N owed=M failed=K)`).
- app-host (`source/services/app-host/src/probe/reconnect.rs`): `OP_SURFACE_CLOSED` → save and
  end; EOF without it → save, wait in the kernel for the parent's notice, re-attach and draw.
- One-instance roles (shell, greeter, IME) are not relaunched; the running instance rejoins
  (`abilitymgr: launch kept (app=…`).

## App lifecycle orders

abilitymgr is the lifecycle authority; execd, the parent, carries out freeze/thaw/kill/squeeze
([RFC-0111](../rfcs/RFC-0111-process-lifecycle-freeze-thaw-kill-the-parent-orders-the-process.md),
[ADR-0082](../adr/0082-a-process-is-its-address-space-the-parent-orders-it-whole-freeze-thaw-kill-and-a-frozen-receiver-refuses-the-park.md),
[ADR-0083](../adr/0083-the-compositor-states-what-is-seen-the-lifecycle-authority-decides-the-parent-orders.md),
[ADR-0087](../adr/0087-a-frozen-apps-private-memory-is-squeezed-on-its-parents-order-the-records-live-with-the-object-a-touch-brings-the-page-back.md)).
Wire and gates: [execd.md](execd.md).
