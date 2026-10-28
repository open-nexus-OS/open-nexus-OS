<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0111: The process lifecycle — a process is its address space, its parent orders it whole (freeze, thaw, kill), the compositor tells the lifecycle authority what is seen, and a frozen receiver refuses the park

- Status: In Progress (Phases 0–3 built and proven on the QEMU lanes and board cycle 72b 2026-10-27; Phase 4 assessed and seeded as RFC-0112)
- Owners: @kernel-team (§3), @runtime (§4–§6)
- Created: 2026-10-27
- Last Updated: 2026-10-27
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6d-0 … 6d-4)
  - ADRs: `docs/adr/0082-…` (the kernel half: a process is its address space, orders, `Frozen`),
    `docs/adr/0083-…` (the lifecycle contract: windowd → abilitymgr → execd), `docs/adr/0073-…`
    (death is an event), `docs/adr/0056-…` (exit reasons)
  - Related RFCs: `docs/rfcs/RFC-0100-…` (§1 the lifetime rule the reap completes; §4 memory
    pressure — 6e builds on this RFC), `docs/rfcs/RFC-0079-…` (EOF, the never-park discipline),
    `docs/rfcs/RFC-0033-…` (nothing waits), `docs/rfcs/RFC-0045-…` (policy as code: the one
    permission authority §6 extends), `docs/rfcs/RFC-0108-…` (the UX budgets a frozen background
    buys)

## Status at a Glance

- **Phase 0 (a process is its address space; a leader's exit takes its threads)**: ✅ 6d-0
- **Phase 1 (freeze / thaw / kill of a child; `killable`; `Frozen`)**: ✅ 6d-1
- **Phase 2 (the lifecycle contract: seen → state → order)**: ✅ 6d-2 — QEMU lanes and board
  cycle 72b green 2026-10-27 (three apps frozen behind the focused one, thawed on return)
- **Phase 3 (one permission authority; the declared background)**: ✅ 6d-3 — QEMU lanes and board
  cycle 72b green 2026-10-27 (7/7 apps published, six launches on the ask; the allow side of
  `background` has no consumer yet: contract + host proof only)
- **Phase 4 (the compositor's restart: ghosts until the next present)**: 🔲 6d-4 → **6g, after
  6f** (the operator, 2026-10-27) — assessed, not built; seeded as RFC-0112 (a service's death is
  a re-resolve for its clients); it serves no package of 6e/6f

Definition: “Complete” means the contract is defined and the proof gates are green on every
QEMU lane and on the board. It does not mean the policy constants never move.

## Scope boundaries (anti-drift)

- **This RFC owns**: the process as the lifecycle unit (§3.1); the three orders, their authority
  and refusals (§3.2); the frozen state and its IPC rule (§3.3–§3.4); the lifecycle states and
  transitions of an app and who decides them (§4); the wire between the compositor, the lifecycle
  authority and the service launcher (§5); the declared background and its one authority (§6).
- **This RFC does NOT own**: memory pressure bands and the kernel's last resort (RFC-0100 §4,
  6e); snapshots and the restore of a killed app's window (6e); the policy profile schema
  (RFC-0091); the compositor's window model (RFC-0067, RFC-0101); scheduling classes (RFC-0023).

### Relationship to tasks (single execution truth)

TASK-0333 6d holds the stop conditions and the proof commands for every phase; this RFC holds
the contract they prove.

## Context

An app that is not seen still costs the foreground: its frames, its CPU, its memory. Before 6d
the kernel had one unit, the task, and one order, `task_resume` of ANY pid; a hidden app ran on,
a crashed leader left its threads alive in a space that could not be freed (the board's
`mappers=3` through every respawn), and nothing could end an app but the app. The two mobile
systems this OS is measured against freeze a hidden app whole within seconds, kill it under
pressure without asking it, keep its window as a snapshot, and make the lifecycle authority — not
the app — the one that decides; both refuse an IPC that would park on a frozen process. 6d
builds that floor: the kernel half as ADR-0082, the policy half as ADR-0083, the permission half
as its own decision.

## Goals

- A hidden app costs nothing but its memory, by the kernel's freeze, not by its cooperation.
- A kill reaps the whole process and frees its space; a crash leaves nothing behind.
- No task in the system can hang on a freeze it did not order.
- The compositor states what is seen; the lifecycle authority decides; the service launcher
  executes. One authority per question, no second copy of the decision.
- The refusal matrix is a boot proof on every lane and on the board.

## Non-Goals

- Thread-wise orders. A thread is not a lifecycle unit.
- Telling the app before a kill. The window's snapshot (6e) is the user-facing state; an app
  that must save state saves it on `Background`.
- A background-execution API beyond the declared `background` grant (§6): no audio, no
  location, no "keep alive" tokens in this RFC.

## Constraints / invariants (hard requirements)

- **Determinism**: every proof is a marker that fires on behavior; the freeze proof carries
  `ran=0` (no hart dispatched a frozen task) and fails loudly otherwise.
- **No fake success**: `KSELFTEST: task freeze ok` fires at the first REAL thaw of a frozen
  task; `SELFTEST: task kill ok` fires at the reap of an ORDERED kill with the kernel's reason.
- **Bounded resources**: the pending order is one field per task; the ordered-kill table is
  eight slots (a ninth kill is executed and reported as a crash — noise, never silence); the
  lifecycle authority's transition queue is `Latest` (one slot per app) and never parks the
  compositor.
- **Security floor**: parentage is the authority of every order; `killable` is set by the
  parent at exec and never by the child; identity on every wire is the kernel's
  `sender_service_id`, never a payload string; the background grant is deny-by-default at the
  one authority.
- **Nothing waits** (RFC-0033): an order for a task on another hart is a mark plus an IPI; a
  send that would park on a frozen receiver is refused.

## Proposed design

### §3 The kernel half (ADR-0082) — normative

#### §3.1 The unit

A process is an address space. The task that created the space is its leader; a task spawned
into its parent's own space is a thread of it. An order names a process and acts on leader and
threads alike. A leader's exit kills its threads (`Killed`, −9) through the one exit funnel; a
thread whose leader is gone is released by its own exit; the leader's reap frees the space.

#### §3.2 The orders and their authority

| Syscall | Number | Authority | Effect | Refusals |
|---|---|---|---|---|
| `task_resume(pid)` | 32 | live child of the caller | thaw / first start of the whole process | `ESRCH` not my child; `EAGAIN` run queue full |
| `task_suspend(pid)` | 65 | live child of the caller | freeze the whole process; returns tasks parked here (+ deferred) | `ESRCH` not my child |
| `task_kill(pid)` | 66 | live child of the caller AND `killable` | kill the whole process (`Killed`, −9) | `ESRCH` not my child; `EPERM` not killable |
| `exec_vmo(…, flags)` | 63 | the spawner | `EXEC_FLAG_KILLABLE` (bit 0) sets the child's `killable` once | as ADR-0080 |

The service launcher spawns every child killable; init spawns no boot service killable. The
kernel's last resort (6e) honours the same bit.

#### §3.3 The frozen state

Frozen is `Suspended` + `frozen`: never enqueued, never dispatched; a wake only clears the
block. An order for a task running on another hart is a pending mark (`Freeze` or `Kill`; a
kill is never downgraded) plus a resched IPI, executed by that hart at its next trap entry
before anything else runs for the task; the frozen task's trap re-executes on the thaw. The
first thaw of a frozen task per boot emits `KSELFTEST: task freeze ok (pid= resumed=1 frozen_ns=
ran=0)`; a dispatch of a frozen task is `TASK: frozen task dispatched`.

#### §3.4 The IPC rule

Every receive endpoint of a frozen process is marked. A send into a marked endpoint whose
queue is full returns `Frozen` (`EDEADLK`, 35; `nexus_abi::IpcError::Frozen`) instead of
parking; an `ipc_call` to a marked endpoint returns `Frozen` at once. What fits the queue is
taken and delivered on the thaw. A compositor, an input chain, a policy daemon: none can hang
on a frozen app.

### §4 The lifecycle states (ADR-0083) — normative

```
Created ─resume─▶ Started ─first present─▶ Foreground ◀─focus/visible─┐
                                              │ hidden or occluded    │
                                              ▼                       │
                                         Background ─grace elapsed─▶ Suspended (frozen)
                                              │                       │  first input / raise
                                              │ system kill (6e)      │  on its band = thaw
                                              ▼                       ▼
                                           Stopped ◀────────────── Foreground
```

- **Who decides**: the compositor (windowd) STATES what is seen — focus, visible, occluded,
  minimized, closed — and never decides a lifecycle. The lifecycle authority (abilitymgr) maps
  the statement to a state and issues the order. The service launcher (execd) executes the
  order on its child (`task_suspend` / `task_resume` / `task_kill`) and reaps.
- **The grace**: `Background` → `Suspended` after a constant number of seconds hidden (the
  authority's constant; the mobile systems use ≈5 s). The constant is a number in one place, a
  budget RFC-0108 will judge, not a tunable in a profile.
- **The wake**: a frozen app is hidden, so the wake is its restore — from the taskbar, from the
  keyboard (Super+Ctrl+M on the bare desktop restores the first hidden window), from a launch
  of the same app. The compositor states `Focused`; the authority thaws (the compositor never
  thaws). What the compositor sent the frozen app meanwhile waited in its outbox (`Frozen` =
  a full queue) and is delivered after the thaw, in order (§3.4).
- **The deaths**: a crash closes the window (recents keep the last snapshot); a system kill
  (6e) leaves a ghost band the user can relaunch from; a user close is a `Stopped` the
  authority orders through the app's own exit first (`OP_CLOSE`, the DSL's quit) and a kill
  after a bounded grace.

### §5 The wire (ADR-0083) — normative, as built in 6d-2

- windowd → abilitymgr: `OP_SEEN` (6) `[A,M,1,6, owner_sid:u64le, seen:u8]` with `seen ∈
  {Focused 0, Visible 1, Hidden 2, Gone 3}` — one-way (no answer), NONBLOCK, one frame per app
  window whose sighting changed, deduplicated against the last sighting the authority took
  (`windowd/src/seen.rs`); a refused send stays owed and goes with the next compositor pass.
  The app is named by the kernel service id of its process (`app:<bundle_id>`), the identity
  the compositor captured at the surface's creation. abilitymgr takes the frame from
  windowd's `sender_service_id` alone (`seen_frame_allowed`). `Hidden` = minimized; a window
  fully covered by another is still `Visible` (occlusion is a follow-up, not a lie).
- abilitymgr → execd: `OP_FREEZE` (4), `OP_THAW` (5), `OP_KILL` (6) `[E,X,1,op, pid:u32le]`,
  answered `[E,X,1,op|0x80, status, pid:u32le]` on execd's response endpoint — execd checks
  the sender is the lifecycle authority (`sender_service_id` = abilitymgr's), checks `pid` is
  a LIVE child, executes the order, records an ordered kill before issuing it (the reap reads
  `Killed` as carried out, not a crash), answers with the kernel's result (`orders::
  order_allowed`, `test_reject_order_from_a_stranger_or_for_a_stranger`). A fourth order of the
  same gate came with TASK-0333 6f-1 (ADR-0087, RFC-0100 §5): `OP_SQUEEZE` (7, `nexus_wire::
  execd`) — the batches of `task_squeeze` (69) over a frozen child until the kernel's walk
  ends, answered with the totals; abilitymgr carries it for the memory authority
  (`OP_MEM_SQUEEZE`) for a suspended instance alone.
- execd → abilitymgr: `OP_INSTANCE_EXITED(pid, code, reason)` (exists, ADR-0073).
- The policy (`abilitymgr/src/seen.rs`, pure): `Focused` → `Foreground`; `Visible` →
  `Background`; `Hidden` → `Background` with the grace clock running from the FIRST hidden
  sighting; the grace out → `Suspended` + `OP_FREEZE`; a `Suspended` instance seen `Focused` /
  `Visible` → thawed into that state; seen `Gone` → thawed into `Background` (it must run to
  exit; the death report ends it). The clock is a one-shot kernel timer on abilitymgr's
  declared pair (`slots::abilitymgr::TIMER`), armed at the earliest grace to run out — no tick.
- The compositor and a frozen app: a send into a frozen app's full queue returns `Frozen`;
  windowd treats it exactly like a full queue (the frame waits in the client's outbox, the thaw
  drains it); the count is on the loop line (`clients(behind= queued_max= dropped=)`). The
  compositor never learns the lifecycle state and never needs to.

### §6 One permission authority — normative

Every app grant has one authority: policyd (ADR-0084). The bundle manager publishes an
installed bundle's manifest grants to policyd (`nexus.permission.*`, the app-owned
`app.<bundle>.<CAP>`) once per boot, from the VERIFIED system volume's `meta/app.properties`
sidecar (`nx app compile` writes `permissions=` and `background=`), before it answers the first
volume request after its policyd leg is live; the lifecycle authority and the service launcher
ASK policyd at every launch and spawn and never read a manifest or a build-time table. Both
build-time `APP_MANIFEST_CAPS` tables are deleted.

The wire (v2 nonce frames of the policyd protocol, `nexus-wire`): `OP_APP_GRANTS_PUT` (10)
`[P,O,2,10, nonce:u32le, app_len:u8, app, n:u8, (len:u8, name)*]`, answered by the v2 status
reply; `OP_APP_GRANTS_GET` (11) `[P,O,2,11, nonce, app_len, app]`, answered by
`[P,O,2,11|0x80, nonce, status, n, (len, name)*]` (`n = 0` unless ALLOW). Bounds: 48-byte ids
and names, 16 grants per app, 32 apps. Who may publish (`app.grants.publish`: bundlemgrd) and
who may read (`app.grants.read`: abilitymgr, execd, the proof harness) is the policy table's,
decided on the kernel sender id. A publish naming a permission the authority does not recognize
(a platform name outside its list, or an `app.*` name not of the shape `app.<bundle>.<CAP>`)
rejects the WHOLE app and removes its earlier entry; a read for an app never published is DENY.
No answer — DENY or no transport — is a refused launch (`abilitymgr: launch denied (grants
app=`) and a child with no service routes (`execd: FAIL app grants (app=`), each a marker.

`background = [...]` in the manifest (`nxb-pack`: at most 8 modes of 1..=32 `[a-z0-9-]`; the
grant may not be named in `caps` directly) is one such grant (`nexus.permission.BACKGROUND`),
deny-by-default: an app without it is frozen after the grace; an app with it stays `Background`
(runnable) until pressure (6e). The allow side has no consumer in 6d (no shipped app declares
it); its contract and its refusal (`test_reject_background_undeclared`) are proven on the host,
the live set is read back every boot (`SELFTEST: app grants ok (app=calculator n= background=0)`,
`SELFTEST: app grants put deny ok`); its runtime effect is not claimed board-proven until an app
uses it.

## Security considerations

- **Threat model**: a process ordering a task that is not its child (confused deputy on pids);
  a child making itself unkillable or killing its siblings; a service spoofing the lifecycle
  authority's orders to execd; a sender parked on a frozen receiver as a denial of service; an
  app claiming a background grant it did not declare.
- **Mitigations**: parentage checked in the kernel for every order; `killable` set by the
  spawner only; execd checks `sender_service_id` against the lifecycle authority's; `Frozen`
  refuses the park; the grant lives at policyd only, deny-by-default.
- **Open risks**: a frozen app holding a shared object another app needs (a clipboard
  transfer, a file picker) — the holder is thawed by the authority on the requester's wake path
  in 6d-2; tracked there.

## Failure model (normative)

- `task_resume`/`task_suspend`/`task_kill` of a non-child: `ESRCH`. `task_kill` of a
  non-killable child: `EPERM`. A thaw whose run queue is full: `EAGAIN` (the task stays frozen;
  the caller retries on its next wake, never in a loop).
- A send that would park on a frozen receiver: `EDEADLK` (`Frozen`); the sender treats it as
  "the receiver is away", never as a transport error (windowd counts `dropped`).
- An `OP_KILL` whose kernel order fails: the recorded order is withdrawn, the failure is
  answered; the pid stays a live child.
- No silent fallback: a lane whose probe child is not killable (a wrong `exec_vmo` flag)
  fails `SELFTEST: task kill ok`.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo +nightly-2025-01-15 test -p execd orders
cd /home/jenning/open-nexus-OS && cargo +nightly-2025-01-15 test -p abilitymgr seen        # 6d-2 (the policy)
cd /home/jenning/open-nexus-OS && cargo +nightly-2025-01-15 test -p windowd seen           # 6d-2 (the sightings)
cd /home/jenning/open-nexus-OS && cargo +nightly-2025-01-15 test -p policyd app_grants      # 6d-3 (the authority)
cd /home/jenning/open-nexus-OS && cargo +nightly-2025-01-15 test -p abilitymgr             # 6d-3 (the ask, the background exemption)
cd /home/jenning/open-nexus-OS && cargo +nightly-2025-01-15 test -p nexus-wire policyd     # 6d-3 (the frames)
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just test-os                      # headless ladder
cd /home/jenning/open-nexus-OS && just ci-os-usb-visible            # 6d-2: the injector's chord, an app frozen behind the focused one (900 s)
```

### Deterministic markers

- `KSELFTEST: process ctl ok (reject=4 frozen=1 frozen_send=1 killed=1)` — the refusal matrix
  and the frozen queue rule against the real syscall table.
- `KSELFTEST: process exit takes threads ok (pid= threads= space=freed)` — the compute
  broker's crash takes its workers; the reap frees the space.
- `KSELFTEST: task freeze ok (pid= resumed=1 frozen_ns= ran=0)` — a RUNNING child frozen, no
  hart ran it, thawed.
- `SELFTEST: exec child freeze thaw ok`, `SELFTEST: task kill ok` — the service launcher's
  probe: a call to the frozen child refused, the ping delivered on the thaw, the ordered kill
  reaped as `Killed` without a crash report.
- 6d-2 (visible lane, the injector's Super+Ctrl+M twice): `windowd: minimize id=app` →
  `execd: freeze (pid=` → `abilitymgr: suspend (inst=` → `execd: thaw (pid=` →
  `SELFTEST: ability lifecycle ok (fg->hidden->frozen->fg)` (once per boot, the first full
  cycle; ASCII — the UART path drops non-ASCII bytes, a marker literal never carries them).
  6d-3 (headless lane, every boot): `policyd: app grants (app= n=)` once per installed app →
  `SELFTEST: app grants ok (app=calculator n= background=0)` → `SELFTEST: app grants put deny
  ok`; every `abilitymgr: launch (app=` and every `execd: app route granted svc=` rides the
  ask. 6d-4: `windowd: restarted (ghosts=`.

## Alternatives considered

- See ADR-0082 (thread-wise orders; a waiting kill; parking on a frozen receiver; cooperative
  sleep as the only mechanism) and ADR-0083 (the compositor deciding the lifecycle; a timer in
  the app; polling the app's state).

## Open questions

- The grace constant: 5 s is the mobile systems' number; RFC-0108's budgets judge it on the
  board (the shell must stay smooth while an app freezes behind it) — owner @runtime, 6d-2.

---

## Implementation Checklist

- [x] **Phase 0**: a leader's exit takes its threads; the reap frees the space — proof:
  `KSELFTEST: process exit takes threads ok (` on every lane; `mappers=1` after the broker's
  respawn on the board.
- [x] **Phase 1**: `task_suspend`/`task_kill`/parent-checked `task_resume`; `killable` at exec;
  `Frozen` — proof: `KSELFTEST: process ctl ok (`, `KSELFTEST: task freeze ok (`, `SELFTEST:
  exec child freeze thaw ok`, `SELFTEST: task kill ok`.
- [x] **Phase 2**: the lifecycle contract (ADR-0083) — proof: `windowd: minimize id=app` →
  `execd: freeze (pid=` → `abilitymgr: suspend (inst=` → `execd: thaw (pid=` →
  `SELFTEST: ability lifecycle ok (fg->hidden->frozen->fg)` on the visible lane (2026-10-27);
  the board's frozen app behind the focused one and its shell smoothness = board cycle 72
  (ack `lifecycle`, open).
- [x] **Phase 3**: one permission authority (ADR-0084); `background` — proof:
  `test_reject_app_grants_from_a_stranger`, `test_reject_app_grants_read_without_cap`,
  `test_reject_unknown_permission_rejects_the_app`, `test_reject_launch_without_published_grants`,
  `test_reject_background_undeclared`, `test_reject_background_out_of_bounds_or_named_directly`
  (host); `policyd: app grants (app= n=)` ×7, `bundlemgrd: app grants published (n=7/7)`,
  `SELFTEST: app grants ok (app=calculator n=1 background=0)`, `SELFTEST: app grants put deny ok`
  (headless + visible lanes 2026-10-27); the board rides every launch on the ask = cycle 72 (open).
- [ ] **Phase 4**: the compositor's restart rung — proof: `windowd: restarted (ghosts=`. Assessed
  2026-10-27, not built: a fleet re-route protocol — seeded as RFC-0112, scheduled as 6g after 6f.
- [x] Task linked with stop conditions + proof commands (TASK-0333 6d).
- [x] QEMU markers appear in `scripts/qemu-test.sh` and the board ladder.
- [x] Security-relevant negative tests exist (the kernel's refusal matrix; `test_reject_*` of
  the services in 6d-2/6d-3).
