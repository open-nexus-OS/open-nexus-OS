<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0086: memd is the one memory authority — it hears the pool, learns the bands, orders a kill through the lifecycle authority — and a restorable app comes back where it was

- Status: Accepted (2026-10-27; the proof is both QEMU lanes and board cycle 73 of TASK-0333 6e — the restore half built in 6e-1 and proven by
  `apphost: persist saved (…)` → `SELFTEST: app restore ok (…)` on the usb-visible lane; the
  memd half built in 6e-2 — the sink bound and the bands pushed on every lane, the kill chain
  `memd: kill` → `abilitymgr: kill` → `execd: kill` → `SELFTEST: mem pressure kill ok` on the
  usb-visible lane's drill; the fan-out built in 6e-3 and proven on the same lane — `memd: level
  told` → `SELFTEST: mem pressure fanout ok (level=warn sent=2)` → `gpud`/`windowd`/`apphost: mem
  pressure (level=warn …)`; the GPU charge built in 6e-4, `gpud: gpu memory charged (client=windowd
  …)` → `memd: gpu charge (…)` on both lanes and on the board — cycle 73: the lent framebuffer and
  the compositor's context buffers charged, the bands of the operator's apps, `persist saved
  (reason=hidden)`; the board's close-and-reopen rung is open with the board's keyboard defect)
- Date: 2026-10-27
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6e-1 … 6e-3)
  - RFCs: `docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md`
    (§3 the pressure event, §6 the restorable contract — this decision fixes both consumers),
    `docs/rfcs/RFC-0111-…` (the lifecycle authority and the parent that orders a process),
    `docs/rfcs/RFC-0072-…` (statefs: the record an app's state lives in)
  - Related ADRs: `docs/adr/0085-…` (the kernel's half: watermarks, one sink), `docs/adr/0083-…`
    (the compositor states what is seen; the sighting an app now hears too), `docs/adr/0084-…`
    (one permission authority: the grant `restorable` derives), `docs/adr/0082-…` (a parent
    orders its process whole — the kill memd asks for), `docs/adr/0036-…` (lifecycle vs process
    vs registry: three services, three truths — memd is a fourth, for memory alone)

## Context

The kernel can now say how tight memory is (ADR-0085: one frame into one sink per level change).
Nobody listens yet, and nothing an app holds survives a kill: the DSL runtime has carried an
`@persist` codec since TASK-0080D (`NXPS` v1, name-keyed, per-entry tolerant) with no host that
writes or reads it, apps have no route to statefsd, and the manifest has no word for "bring me
back". The phones this OS is measured against decided both a decade ago: a daemon owns the
pressure policy (bands from the window manager, levels from the kernel, kills from the bottom
band up, never a service, never the foreground), and an app that saved its small state is
relaunched into it so the kill was never seen.

Three forks were on the table for the restore half. (1) Where the record lives: a file the app
opens (no app opens storage here, RFC-0072), a per-app key in statefsd by bundle name in the key
(a payload string names the owner — forgeable), or a per-app key whose owner IS the sender's
kernel identity. (2) When it is written: on every store change (a journal append per keystroke —
and a privacy leak in the trace), on a timer (the clock pattern this repo retired), or on the
compositor's `hidden` sighting (the grace before a freeze, ADR-0083) and at the window's close.
(3) Who grants it: a cap in `caps` (the authority already knows `nexus.permission.STATE`, listed
and never granted), or a manifest word the publisher derives the grant from — the shape
`background = [...]` took in ADR-0084.

## Decision

- **`restorable = true` is the manifest's word.** nxb-pack reads it as a boolean (a `caps` entry
  naming the grant directly is a pack-time error, as for `BACKGROUND`); `nx app compile --meta`
  writes `restorable=` into the sidecar; bundlemgrd derives the one grant
  `nexus.permission.STATE` at publish; execd provisions the SDK route `svc.state` → statefsd into
  the child's fixed slot 25 for that grant alone. **The route is the capability**: an app without
  it has no slot and nothing to ask.
- **The record is the sender's, by kernel identity.** `/state/app/@self/…` — statefsd rewrites
  `@self` to the sender's `sender_service_id` (the launcher-derived `app:<bundle>` sid, stable
  across launches) before any rule runs; the owner's own record needs no policyd ask (the route
  was the ask); another owner's record by name is foreign — the generic `statefs.*` grant decides,
  which no app holds. `test_reject_app_key_of_another_owner` on the host.
- **The snapshot is written when the compositor says hidden, and at close.** The compositor
  pushes its sighting of the window to the window itself (`OP_SURFACE_SEEN` 35: focused /
  visible / hidden — the same sighting it states to the lifecycle authority, ADR-0083). On
  `hidden`, app-host writes the runtime's `@persist` snapshot (bounded: 2048 bytes, said when
  over) — inside the five-second grace, before the authority may freeze the process and memd may
  kill it. At the window's close (the compositor released the event channel) it writes once more,
  then exits. Never per change, never on a clock: `apphost: persist saved (fields= bytes=
  reason=hidden|close)`.
- **The snapshot is read once, before the initial effects.** A program with a persisted field
  asks its record at mount; the runtime restores name-keyed, per entry; the root effects run after
  it, so a field a service fills still refreshes over the stale copy. `SELFTEST: app restore ok
  (fields= bytes= warm_us=)` when at least one field came back.
- **memd is the one memory authority (6e-2, built).** A platform-stage service on the system
  volume (`source/services/memd`, `ServiceId::Memd`), it binds the kernel's pressure sink to its
  declared endpoint pair (`NamedSlot::MemPressure{Recv,Send}`, minted by init like a child-death
  endpoint) and waits on that endpoint and its server endpoint — no clock. The kernel's first frame
  arrives at the bind: `memd: pressure (level= free= total=)` is the sink's live proof on every
  lane. **The bands come from the lifecycle authority as a diff**: after every change the broker
  went through, abilitymgr pushes each app process whose band differs from what memd last heard
  (`OP_BAND`: foreground 0 — the focused window's process and one still starting — > visible 1 >
  declared 2 — hidden with `nexus.permission.BACKGROUND` — > hidden 3 > suspended 4; `gone` when
  the reaper's death report removed the instance), NONBLOCK on the declared leg, committed only
  when taken. memd takes a band from abilitymgr's kernel sender id alone. **The kill rule**: at
  `critical` the floor is `hidden` (nothing on screen goes), at `emergency` `visible` (the
  foreground never); the victim is the highest band at or above the floor, the longest in it
  first; one kill in flight at a time. **The order goes through the lifecycle authority**:
  `OP_MEM_KILL {pid}` to abilitymgr, taken from memd's kernel sender id alone, refused for a
  foreground or starting instance (`test_reject_mem_kill_of_foreground`), carried to the parent
  as execd's `OP_KILL` (ADR-0082): `memd: kill (pid= band= reason=)` → `abilitymgr: kill (inst=
  reason=memory)` → `execd: kill (pid=)`. **The proof is the death**: the kill is proven when the
  victim's `gone` band arrives — `SELFTEST: mem pressure kill ok (band= reason=oom|drill)` — never
  at the order; a kernel-sourced kill then reads the level again (the kernel sends one frame per
  level CHANGE, a pool still at `critical` needs the next victim). memd never kills, never a
  service (only processes the lifecycle authority named are in its table), never the foreground.
- **The lane proves the chain with a drill, the real level is the `lowmem` lane's.** The proof
  harness, on a QEMU lane with a display only (never on a board: it would end an app the operator
  is using), asks memd's `STATUS` once a second in its end phase; once a frozen app has run again
  (the lifecycle rung) and an app is frozen once more (the injector hides the restored window), it
  sends `OP_DRILL {critical}` — accepted from the harness's kernel sender id alone — and memd runs
  the same policy over the real bands: the victim, the order, the kill and the reap are the live
  path, only the level is simulated (`reason=drill`). `SELFTEST: memd bound ok (level= bands=)` on
  every lane says the sink is memd's; `reason=oom` belongs to 6f, where a `lowmem` profile runs
  the pool low for real.
- **The level told onward (6e-3, built and lane-proven 2026-10-27).** Every level memd learns — the kernel's and
  a drill's — goes out as `nexus_wire::memd::OP_LEVEL {level, source, free, total}` on two
  declared legs (`REQUIRED_ROUTES` memd→gpud, memd→windowd), NONBLOCK: the authority never waits
  on a consumer, a full queue drops the frame and the next change tells the truth
  (`memd: level told (level= source= gpud= windowd=)`). gpud takes it from memd's sender id alone
  and names it (`gpud: mem pressure (level= source=)`; what it drops waits for the per-client
  ledger, 6e-4 — on QEMU its glass textures are the host GPU's memory, not pool frames). windowd
  takes it the same way and tells every live app window `OP_SURFACE_PRESSURE {level}` (36) on the
  window's own event channel (`windowd: mem pressure (level= source= windows=)`); app-host raises
  it into the program as the host event `MemoryPressure` (`Normal`/`Warn`/`Critical`/`Emergency`,
  no payload — `apphost: mem pressure (level= handled=)`); a program that declares no case does no
  work. The kernel's own answer to `warn` — the purge of the objects their owners unlocked
  (RFC-0100 §4, syscall 68) — needs no word from memd: it runs before the pressure frame leaves.
  Proof: the drill has two arming conditions — `warn` once a thawed app is on screen (a frozen
  window cannot hear the level; `SELFTEST: mem pressure fanout ok (level=warn sent=2)`, `apphost:
  mem pressure (level=warn handled=)`), `critical` once an app is frozen again (the kill).
- **Out of scope here**: the record's size class beyond "durable small" (RFC-0100 §6: documents
  are the query service's), a journal of memd's kills (6f), the compositor's ghost for a killed
  window (6g), a snapshot on every transition the authority knows (the compositor's sighting is
  the one app-host can hear).

## Consequences

- **Positive**: a kill under pressure returns the user to where they were, with no code in the
  app beyond `@persist` on a field; the owner of a record is a kernel fact, never a string;
  nothing is written while the user types (privacy: no keystroke timing in the journal, no
  content in the trace — the marker carries counts); the authority's shape stays one (a manifest
  word → a derived grant → a provisioned route).
- **Negative / accepted cost**: a hidden app that is killed inside its first five seconds of
  hiding loses the fields changed since the previous hide or close (the grace is the authority's
  constant; a kill before it is an emergency, 6e-2); a second compositor push per sighting (one
  byte); the chord table gains Super+Ctrl+W (close) — the keyboard's close for the proof and the
  operator, the window's own button unchanged.
- **Negative / accepted cost (memd)**: a process the lifecycle authority never named (a
  window-only launch, a service) is invisible to the authority — never a victim, never protected
  by a band either (the kernel's emergency last resort stays the kernel's); the band table is
  bounded at 32 like the broker's; one kill in flight means a pool that stays at `critical`
  takes its victims one reap apart, not in a burst — deliberate (a burst kills what one death
  would have freed).
- **GPU memory charged to its client (6e-4, built and lane-proven 2026-10-27).** gpud tells memd what it holds
  FOR each client — the framebuffer it lent, the client's context buffers on the board — as an
  absolute `OP_GPU_CHARGE {client, bytes}` per change (NONBLOCK; zero frees the row; eight
  clients, a ninth refused by name); memd takes it from gpud's sender id alone into its own table
  (`memd: gpu charge (client= bytes= total=)`). The authority's picture of a process now has the
  GPU's side; the join to a pid comes when a client is an app (layer 8).
- **Follow-ups**: what gpud drops at `warn` (the board's glass cache is a GPU-private buffer of
  the one pool — purgeable now that the ledger says what it costs whom; QEMU's textures are the
  host GPU's); the first purgeable owners (atlas bands, blur cache);
  6f (memd's journal, the `lowmem` lane where a real level change drives a real kill —
  `reason=oom` — and the restore closes the loop); every shipped app with navigation or draft
  state marks it `@persist` and says `restorable = true`.

## Alternatives considered

- A snapshot on every store change — a journal append per keystroke, a timing side channel in
  the trace, and the pattern `measurement-privacy` forbids; the sighting is the one moment that
  matters.
- A snapshot on a timer — the clock pattern this repo retired (ADR-0073, `check-wait-not-poll`).
- The bundle name in the key — a payload string names the owner; any app could name another's.
- `nexus.permission.STATE` in `caps` — the manifest would state a grant instead of a need, and a
  pack-time ceiling could not tell "wants its state back" from "wants statefs"; the derived grant
  keeps one shape with `background`.
- The lifecycle authority tells the app to save (an `OP_SAVE` from abilitymgr through execd) — a
  third service in the path for a byte the compositor already has; and the authority never talks
  to an app (ADR-0083: it talks to the parent).
- memd kills by itself (`task_kill` on a pid it is told) — two orderers of a process; ADR-0082
  has one (the parent), ADR-0083 one decider (the lifecycle authority). memd asks, abilitymgr
  orders, execd does.
