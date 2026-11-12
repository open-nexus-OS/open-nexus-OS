---
schema: 1
title: execd
role: guide
subsystem: app-platform
sources: [source/services/execd, source/kernel/neuron/src/syscall/api/exec.rs, source/libs/nexus-wire/src/execd.rs]
contracts: [ADR-0080, ADR-0081, ADR-0091, ADR-0073, RFC-0081, RFC-0111]
verified: 2026-11-12
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# execd

> **Purpose** — how a process other than a boot service is made, granted, reaped and reported.
> **Owns** — `source/services/execd/`; the kernel half is `exec_vmo` in `source/kernel/neuron/src/syscall/api/exec.rs`
> **Contracts** — [ADR-0080](../adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md), [ADR-0081](../adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md), [ADR-0091](../adr/0091-no-ambient-authority-over-address-spaces-a-task-maps-and-spawns-only-into-its-own-the-kernel-bounds-what-a-caller-sizes.md), [ADR-0073](../adr/0073-an-object-dies-with-its-last-capability-a-death-is-an-event-on-an-endpoint.md), [RFC-0081](../rfcs/RFC-0081-process-reaper-nonblocking-reclaim.md), [RFC-0111](../rfcs/RFC-0111-process-lifecycle-freeze-thaw-kill-the-parent-orders-the-process.md)
> **Invariants** — `exec_vmo` from a sealed image is the only way a process is made; every grant lands before `task_resume`; the requester's identity is the kernel `sender_service_id`, never the name in the frame.
> **Read next** — [lifecycle.md](lifecycle.md), [nexus-init.md](nexus-init.md), [../services/bundlemgrd.md](../services/bundlemgrd.md)

## Role

execd is the parent of every app process (app-host) and of the probe children the selftest
spawns. Boot services are init's children, not execd's. execd:

- spawns from **sealed images** it holds, one object per image id;
- installs a child's capabilities, then resumes it;
- reaps its children when the kernel reports a death and tells the lifecycle authority;
- writes crash evidence;
- carries out lifecycle orders (freeze, thaw, kill, squeeze) from abilitymgr;
- tells every app that a restarted compositor is back.

It has no ELF loader of its own and no `nexus-loader` dependency.

## The one exec path (kernel)

| Rule | Source |
|---|---|
| `exec_vmo` (syscall 63) makes a process from a sealed VMO: read-only and executable segments mapped from the image's own frames (shared by every process of that image), data copied, `.bss` and stack lazy. The task starts suspended. | ADR-0080 |
| The pointer-and-length loaders (syscalls 13 and 17, among them `exec_v2`) are deleted, their numbers retired. | ADR-0080 |
| `spawn` (7) starts a thread only in the caller's own address space; `as_create`/`as_map` are retired. | ADR-0091 |
| The bootfs files are born sealed; init hands execd the app-host's and recv-wake probe's objects in declared slots at its spawn. | ADR-0081 |
| `killable` is the parent's one-time choice at `exec_vmo`: execd marks every child killable, boot services are not. | ADR-0082 |

## Images

Image ids map to sealed objects in `src/os_lite/images.rs`:

| Id | Image | Source |
|---|---|---|
| 1 | hello | embedded probe (`userspace/exec-payloads`) |
| 2 / 3 | demo.exit0 / demo.exit42 | embedded probes (`source/apps/demo-exit0`) |
| 4 | app-host | bootfs file (empty slot ⇒ `UNSUPPORTED`) |
| 5 | fault probe (kernel kills it with `Fault`) | embedded |
| 6 | VMO holder (exits holding a VMO) | embedded |
| 7 | cache probe (illegal `cbo.clean`) | embedded |

Embedded probes are sealed at first spawn and reused. An app-host gets an 8 MiB lazy stack.

## Wire (`[E, X, ver, op, …]`)

| Op | Name | Sender | Effect |
|---|---|---|---|
| 1 | `OP_EXEC_IMAGE` | abilitymgr, selftest | spawn image id, optional app id |
| 2 | `OP_REPORT_EXIT` | a minidump child | validated crash metadata |
| 3 | `OP_WAIT_PID` | spawner | exit code + reason (answered from the reap cache) |
| 4–7 | `OP_FREEZE` / `OP_THAW` / `OP_KILL` / `OP_SQUEEZE` | abilitymgr only, live child only | `task_suspend` / `task_resume` / `task_kill` / `task_squeeze` |
| 8 | `OP_COMPOSITOR_BACK` | init only | re-pin apps' windowd slots, notify each app |

Order gate: `src/orders.rs` (host-tested, `test_reject_*`). Shared codec:
`source/libs/nexus-wire/src/execd.rs`.

## Spawn flow (`OP_EXEC_IMAGE`)

1. Bound and decode the frame; reject malformed.
2. Requester name must hash to the kernel `sender_service_id` (`execd: spawn denied (id mismatch)`).
3. Exec check through init's control channel → policyd (`execd: spawn denied (policy)`).
4. App-host only: fetch the app's permission grants (RFC-0111 §6), then arm a payload object at
   bundlemgrd (`GET_PAYLOAD`): a lazy VMO of `nexus_wire::payload_vmo::VMO_LEN` (a 16 MiB bound,
   ADR-0093) — the bundle's own pages are what the launch commits.
5. `exec_vmo(image, stack, 0, name, killable=true)`; an app runs as `app:<bundle_id>`
   (RFC-0086 identity).
6. Grants before resume: windowd route, payload VMO, event channel (execd keeps its SEND half),
   timer channel, read-only shared glyph atlas (RFC-0080), the app's granted service routes
   (`nexus-sdk-routes`).
7. Wait for bundlemgrd's payload answer, close execd's payload capability, `task_resume`.

## Reaping and death

- At start execd binds a child-death endpoint (`child_exit_bind`, syscall 62, ADR-0073)
  (`execd: reaper armed`). The kernel writes one `OP_CHILD_EXITED` frame per dead child.
- The wake runs a sweep of `wait_nohang_with_reason` (RFC-0081): every ready zombie is reaped
  (`execd: reaped pid=<p> code=<c> (free= vmos= …)`) with its kernel exit reason (ADR-0056).
- An app's death goes to abilitymgr as a one-way `OP_INSTANCE_EXITED`
  (`execd: exit notified (pid= to=abilitymgr)`), bounded pending queue, never a blocking request.
- An ordered kill (ADR-0082) is recorded first, so the reap reads `Killed` as the order carried
  out, not a crash.

## Crash evidence

A fault, a kill not ordered, or a non-zero exit is a crash:

- `execd: crash report pid=<p> code=<c> name=<n>`;
- structured event to logd via `nexus-log` ([RFC-0011](../rfcs/RFC-0011-logd-journal-crash-v1.md));
- minidump converted to `.nxcd` under `/state/crash/` (`src/crash_os.rs`, `src/crash_store.rs`,
  [RFC-0031](../rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md)); attachment level from
  policyd, deny-by-default.

## Host backend

`src/std_server.rs` serves the request protocol over a transport for host tests and execs
nothing (every exec answers unsupported). The os-lite `exec_elf(…, _restart)` is a stub that
returns `Unsupported`; its restart argument is ignored — restarts are init's job
([lifecycle.md](lifecycle.md)).

## Debugging

| Symptom | Look at |
|---|---|
| `init: up execd` but no app launches | `execd: spawn denied (…)`, `execd: FAIL app payload (…)` |
| App never draws | grants before resume (`execd: FAIL app route …`), `execd: FAIL child resume` |
| Zombies accumulate | `execd: reaper armed` missing; `execd: reaped` lines |
| Crash not recorded | `execd: crash logd …` lines, statefs reachability |
