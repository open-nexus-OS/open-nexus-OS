<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0093: Display handoff and boot-stage contract — routing v2, readiness verbs, stage fence, ONE slot topology, handoff v2

- Status: Draft (Phase 1 seed 2026-09-09 — execution TASK-0324 P2–P9)
- Owners: @runtime @gpu @ui
- Created: 2026-09-09
- Last Updated: 2026-09-09
- Links:
  - Tasks: `tasks/TASK-0324-display-handoff-deterministic-by-construction.md` (execution + proof, P0–P9)
  - ADR: `docs/adr/0062-boot-stage-fence-and-readiness-barriers.md` (the one synchronization decision this contract rests on)
  - RFCs: `docs/rfcs/RFC-0069-init-declarative-service-manifest-slot-discipline-boot-stages.md` (the manifest spine; §4 "boot stages" is implemented here), `docs/rfcs/RFC-0066-production-grade-service-chain-declarative-routing-typed-ipc-inprocess-tests.md` (declarative routes), `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` (init markers A1–A4), `docs/rfcs/RFC-0074-display-mode-authority-fwcfg.md` (display-mode SSOT), `docs/rfcs/RFC-0068-structured-event-observability-subject-grouped-journal-renderer.md` (UART folding)
  - ADRs: `docs/adr/0041-never-black-boot-splash-atomic-desktop-reveal.md` (atomic desktop reveal — kept, mechanism replaced), `docs/adr/0050-display-mode-authority.md` (display-mode authority — the query chain is retired), `docs/adr/0057-service-restart-capability-re-resolve.md` (`STATUS_STALE`), `docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md` (kernel timeline fence)
  - Docs: `docs/testing/os-markers.md` "Display truth", `docs/architecture/09-nexus-init.md`, `docs/architecture/06-boot-and-bringup.md`

## Status at a Glance

- **Phase 0 (build truth + honest GL path + `visible` pixel lane)**: ✅ 2026-09-09 (TASK-0324 P0)
- **Phase 1 (this contract + ADR-0062)**: ✅ 2026-09-09 (TASK-0324 P1 — paper)
- **Phase 2 (`@ready` verb, honest `init: up`)**: ✅ 2026-09-09 (TASK-0324 P2 — `test-all` green over 8 lanes, 0 announce failures, `init: up` follows `<svc>: ready` everywhere)
- **Phase 3 (routing v2: nonce mandatory, parked replies, fail-closed)**: ✅ 2026-09-10 (TASK-0324 P3 — `test-all` green over 8 lanes; park scope amended below with implementation evidence)
- **Phase 4 (ONE slot topology crate; every bespoke init arm deleted)**: ✅ 2026-09-12 — the crate, the declared arm and every consumer: windowd (P4a), inputd (P4b), gpud + the fleet-wide MMIO slot (P4c), hidrawd (P4d), the app-child table (P4e), execd (P4e-2), the generic arm's fifteen services + the block plane (P4f-1a/1b), policyd + keystored (P4f-2), updated + bundlemgrd (P4f-3), netstackd + dsoftbusd + metricsd (P4f-4), the proof harness (P4f-5) and the closure (P4f-6). 191 → 0 positional slot declarations; `check-slot-ssot.sh` is an absolute gate that proves its own scanner; init has neither an order-based capability transfer nor a clone taken for one left; `just test-all` green in one run per package
- **Phase 5 (stage fence replaces every `yield_()` sync)**: ⬜ TASK-0324 P5
- **Phase 6 (handoff v2: reveal handshake, seq acks, kernel display mode, readback off-scanout)**: ⬜ TASK-0324 P6
- **Phase 7–9 (consumer polls deleted, closure docs/gates, 8/8 boots)**: ⬜ TASK-0324 P7–P9

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

- **This RFC owns**: the routing frame v2 and its reply-parking semantics; the readiness verbs (`@ready`, `@stage`) on the init control channel; the boot-stage fence values and the fence-capability handout; the slot topology as ONE declarative source for init, services and app children; the windowd↔gpud handoff contract v2 (attach ack, reveal handshake, present sequence acks); the rule that the display mode has exactly one source; and the marker/proof contract for display truth.
- **This RFC does NOT own**: the compositor/present pipeline (scene graph, damage, layers, cursor compositing — RFC-0059/0067); the session/greeter design (only the poll mechanics go, TASK-0324 P7); build truth (delivered P0, documented in `docs/testing/display-output-hardening-matrix.md`); supervision/restart semantics (RFC-0087, ADR-0057); the network family; QEMU/virglrenderer behaviour.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- TASK-0324 executes every phase, one package per phase, each package atomic per consumer (an init arm and its consumer change in the same package) and each package re-proving its blast radius (volume boot, OTA lanes incl. `bundle reused`, reliability, ingress/egress, folding).

## Context

`just start` showed a black window with every marker green (2026-09-09). Build truth and the
2D-on-GL scanout were fixed in P0. Underneath, the display chain is synchronized by luck:
positional slots hardcoded on both ends of every pair; nonce-less route queries whose answers
can be misread by the wrong waiter (windowd needed alias guards, packagefsd silently fell back
to a RAM seed, TASK-0321 P5); `init: up <svc>` printed for a process that was merely resumed;
`yield_()` and driver resume order used as barriers; a reveal decided by a 3-pixel probe under
time caps; present acks under a lease with a "STALL" recovery; the display mode asked from the
device at three places with retry loops. Every one of these is a race by construction on SMP,
and every one has a witness in the 2026-09 logs (`touchd` never ran to ready in proof boots;
1/16 silent stalls; 1/14 silent policy denies from a missing reply).

## Goals

- **Readiness is a message, not a print.** A service is ready when it says so on its control
  channel; init records it; `init: up` is emitted only then.
- **Routes are answered exactly once, to the right waiter, when the target exists.** Nonce is
  mandatory; a route to a not-yet-ready target is parked, never polled and never faked.
- **Stages are a kernel fence.** Platform < DisplayReady < SessionStart < ShellVisible, signalled
  by init from `ready[]`/`@stage`, awaited by children with a WAIT-only fence capability at a
  declared slot. No `yield_()` synchronization anywhere in init.
- **One slot topology.** `ServiceId`, `ServiceSpec` with server/reply/route/extra slots and stage
  live in ONE crate that init, every service and every app child compile against; no positional
  slot literal survives outside generated constants.
- **Handoff v2.** The attach ack carries the mode and content rect; the desktop is revealed by an
  explicit `OP_REVEAL` handshake; every present carries a sequence number that its ack echoes;
  the display mode has one source (`boot_display_mode()`); pixel proof is the display gate.

## Non-Goals

- Changing what the compositor draws or how (scene graph, damage, GPU layers).
- A generic "service mesh" or dynamic service discovery beyond `samgrd` (RFC-0066).
- 2D-device path changes (`virtio-gpu-device`): it stays the only path for non-GL devices,
  selected by `scanout_policy.rs` (P0) — not a dual structure.

## Constraints / invariants (hard requirements)

- **Determinism**: no timing caps decide correctness. A reveal, a route answer, a stage signal
  each happen on an explicit event or the boot fails loudly.
- **No fake success**: `init: up`, `stage: …`, `display: first scanout ok`, `gpud: desktop
  reveal` are emitted only when the named event was observed.
- **Bounded resources**: parked routes ≤ `PARK_MAX = 32`; ready table = `ServiceId::COUNT`
  bits; one fence per boot; control-channel replies are bounded-blocking with the queue depth
  as the only wait bound; present sequence window ≤ 64 outstanding.
- **Security floor**: identity from `sender_service_id`; the fence capability handed to children
  carries WAIT rights only (a child can never signal a stage); route answers are policy-checked
  fail-closed (policy unreachable ⇒ deny + `!route-deny (policy unavailable)`).
- **No dual structure**: each phase deletes the mechanism it replaces in the same package and
  adds a structure gate (`check-slot-ssot.sh`, `check-init-sync.sh`, grep gates in tests) so the
  old form cannot be built into again.
- **Stubs policy**: none. A headless boot reaches `@ready`/`@stage` through the same verbs.

## Proposed design

### 1. Routing v2 (normative wire; replaces the v1 optional-nonce extension)

Frames keep the `'R','T'` magic and `VERSION = 1`; the nonce becomes mandatory and rides in the
reply. Layout (all integers little-endian):

| Frame | Bytes |
|---|---|
| `ROUTE_GET` | `[R, T, 1, OP_ROUTE_GET=0x40, name_len:u8, name[name_len], nonce:u32]` — `name_len` 1..=48; frames without the nonce ⇒ `STATUS_MALFORMED` + `!route-malformed` |
| `ROUTE_RSP` | `[R, T, 1, OP_ROUTE_RSP=0x41, status:u8, send_slot:u32, recv_slot:u32, nonce:u32]` — nonce echoed; a reply whose nonce is not the waiter's is a protocol error at the waiter (`!route-nonce-mismatch`), never consumed |

Status codes are unchanged (`OK=0, NOT_FOUND=1, MALFORMED=2, DENIED=3, STALE=4`) with tightened
semantics:

- An ask whose target is momentarily **unresolvable** — the supervisor marked it stale and is
  restarting it (ADR-0057) — is **parked** in init (`route_park.rs`: bounded ring of
  `{chan, target, nonce}`, `PARK_MAX = 32`) and answered **exactly once** when the route is
  re-provisioned (`init: route resumed svc=… -> …`). A re-ask from the same requester replaces
  the parked entry (its nonce superseded the old one); a requester that exits loses its parked
  asks; overflow is loud (`init: FAIL route park overflow svc=… -> …`) and falls back to a
  `STATUS_STALE` answer rather than dropping the ask. The client never re-asks in a loop —
  that loop was the storm that filled init's control queue (TASK-0324 P2 finding).
- **Amendment 2026-09-09 (P3, implementation evidence).** The Phase-1 seed said an ask is
  parked until the target announces `@ready`. That is wrong by construction and is not
  implemented: readiness is a *service-side* fact, and making a synchronous route ask wait for
  it is cyclic — `bundlemgrd` asks for `metricsd`, and `metricsd` boots from the very system
  volume `bundlemgrd` serves; the same shape exists for every service that resolves a route
  lazily inside its serving loop. Routing therefore waits only on init's OWN bookkeeping
  (route provisioning), and ordering by readiness is the stage fence's job (§3, P5). Endpoints
  are queues: a client that resolves a route before the target serves does not lose messages,
  it fills a bounded queue — which is what the stage fence exists to order.
- `STALE` reaches a client only when init could not park (overflow). It means "registered but
  dead" (ADR-0057) and is **terminal for that ask**: the client decides, it never re-asks in a
  loop. The library's former STALE re-ask loop is deleted.
- `DENIED` covers policy denial AND policy unavailability (`policyd_route_allowed` is
  fail-closed: unreachable ⇒ deny, witness `!route-deny (policy unavailable)`).
- Replies are sent bounded-blocking on the control channel; a reply that cannot be delivered
  within the queue drain bound is `init: FAIL route reply drop svc=…` (loud), never a silent loss.

Deleted with this phase (P3): `query_route` v1 (nonce-less ask + its 32-frame stale-reply
drain + `ROUTE_QUERY_TIMEOUT`), the client-side STALE re-ask loop, and windowd's
`SERVER_RECV_SLOT`/`ALIAS_REPORTED`/`note_server_recv_slot` alias guards (both the bind guard
and the drain guard). windowd's wired-slot fallback is positional-slot structure and belongs to
P4. Gates: `route_park` unit tests (incl. `test_reject_park_overflow_is_loud`) and
`scripts/check-init-sync.sh` — no `query_route` anywhere, no alias guards in windowd, and the
responder never builds a route reply outside `route_reply::send_route_rsp`.

### 2. Readiness verbs on the init control channel (normative)

The control channel already carries ASCII verbs (`@reply`, `@mint-pair`). Two one-way verbs are
added, same framing (verb text, no payload unless stated):

| Verb | Direction | Meaning |
|---|---|---|
| `@ready` | child → init | the sender's `entry()` reached its serving loop. `nexus_service_entry::ready(marker)` prints the service's `<svc>: ready` marker AND sends `@ready` (one call, one place; `check-init-sync.sh` rejects any other `": ready"` print). |
| `@stage <name>` | windowd → init | `display-ready` after the P6 reveal ack; `shell-visible` after the shell's first presented frame. Only windowd may send it (identity check); others ⇒ `!stage-deny`. |

init keeps `ready[ServiceId]` (`ready_table.rs`), sets it on `@ready` from the pid it spawned
for that id (`test_reject_ready_from_unknown_pid`, `test_reject_double_ready_without_exit`),
clears it on exit (supervision sweep), and emits `init: up <svc>` **only** from that handler
(RFC-0013 A1 becomes literally true: `init: up` = init observed `@ready`). Marker order in proof
manifests: `<svc>: ready` precedes `init: up <svc>`.

### 3. Boot-stage fence (normative; ADR-0062)

One kernel timeline fence per boot (`fence_create` / `fence_signal` / `fence_wait`, syscalls
41–43). Values are monotone:

| Stage | Value | init signals when |
|---|---|---|
| `Platform` | 1 | every CORE service (policyd, virtioblkd, bundlemgrd, statefsd, samgrd, …) is `ready[]` |
| `DisplayReady` | 2 | every display service in the boot graph is `ready[]` AND windowd sent `@stage display-ready` |
| `SessionStart` | 3 | `samgrd` registry populated AND the bootctld handshake completed |
| `ShellVisible` | 4 | windowd sent `@stage shell-visible` |

- `ServiceSpec.stage: Stage` names the stage a service belongs to; `nexus_service_entry::os::
  bootstrap` calls `fence_wait(STAGE_FENCE_SLOT, spec.stage.prerequisite(), 0)` before
  `entry()` (Platform services have no prerequisite). The fence capability is handed to every
  child at the declared extra slot `StageFence` with **WAIT rights only**; if the kernel's cap
  transfer cannot express that mask today, ADR-0062 scopes the kernel change and its proof
  (`KSELFTEST: fence transfer ok`).
- Recovery/safe graphs derive their barrier sets from `boot_graph::includes`; an empty set
  signals immediately (recovery reaches `DisplayReady` without a display).
- The existing `stage: display-ready` / `stage: session-start` markers move to the signal sites
  (strings unchanged); `stage: shell-visible` is added.
- Deleted (P5): every `yield_()` used as a barrier in init, the ordered `resume_drivers` list
  (order becomes irrelevant: dependencies are parked routes + the fence). Gate:
  `check-init-sync.sh` rejects `yield_()` in `source/init/nexus-init/src`;
  `test_reject_stage_order_monotonic`, `test_reject_stage_barrier_depends_on_excluded_service`,
  `test_reject_route_to_later_stage_without_park`.
- Liveness witness: a quiet-system timeout (every hart idle, nothing runnable, tasks blocked)
  prints a kernel snapshot (`KSELFTEST: liveness snapshot …`) — the open 1/16 stall evidence
  item of P0 gets a kernel witness before P5 may claim determinism.

### 4. ONE slot topology (normative)

Crate `source/libs/nexus-service-topology` (`no_std`, `forbid(unsafe_code)`): `ServiceId`,
`Route`, `ServiceSpec { id, exposes_server, reply_inbox, routes_to, announce, stage,
server_slots: SlotPair, reply_slots: Option<SlotPair>, extra_slots: &[NamedSlot] }`,
`REQUIRED_ROUTES`, `SERVICE_SPECS` for every service, `route_slots(from, to) -> SlotPair`,
`extra_slot(svc, NamedSlot) -> u32`. Named extra slots cover MMIO (48), IRQ notify, settings
watch (0x40/0x41), settings (0x20–0x22), the stage fence. `nexus-sdk-routes` (app children) keeps its API and its own child-slot space (a per-app
capability table, not the service one) but is JOINED to this crate by
`test_reject_sdk_routes_diverge_from_topology`: every `svc.*` row must name a declared
service. Slots migrate ONE CONSUMER PER PACKAGE; until a consumer is migrated its slots read
`SlotPair::UNDECLARED`, `test_reject_partial_slot_declaration` fails a half-declared service,
and `scripts/check-slot-ssot.sh` ratchets the remaining positional declarations down
(they may shrink, never grow). init provisions every declared slot through one generic arm
(`declared_slots.rs`, `cap_transfer_to_slot`); services and drivers compile against generated
constants. Deleted (P4a–P4f, atomic per consumer): every `new_with_slots(…)`, every
`*_SLOT: u32 = <literal>` outside generated files, every `"<svc>" =>` arm in `wiring.rs`,
`is_bespoke_wired`, the slot-order comments. Gate: `check-slot-ssot.sh` in `just check`;
`test_reject_slot_collision_per_service`, `test_reject_route_without_slots`,
`test_reject_service_missing_from_specs`, `test_reject_clone_leak`.

- **Amendment 2026-09-10 (P4a-P4e-2, implementation evidence).** "Every `"<svc>" =>` arm in
  `wiring.rs` is deleted" is too broad and is not what the migrations do. What a bespoke arm
  holds is two different things: WHICH slot a capability lands in (topology — deleted from the
  arm, declared once, pinned by `declared_slots.rs`) and WHICH capability is handed over at all
  — clone vs. move of a shared endpoint, an endpoint minted per service (execd's recv-wake
  probe), a route recorded for later name resolution. The second kind is per-service
  provisioning, not slot topology, and moving it into a generic table would only rebuild the
  same decisions behind an indirection. So the arms shrink to their provisioning decisions and
  move to their own modules (`bootstrap/execd_wiring.rs`), the slot decisions leave them
  entirely, and `is_bespoke_wired` stays as the gate that keeps the GENERIC arm off a service
  that provisions itself. The mechanical guarantee is the ratchet, not the arm count:
  `check-slot-ssot.sh` (191 → 134 positional declarations so far) plus
  `test_reject_partial_slot_declaration`.
- **Amendment 2026-09-11 (P4f-1b, implementation evidence).** The route model gains a third
  delivery kind, `PrivateInbox { inbox_send }`: replies return on an inbox that belongs to ONE route
  (request SEND at `slots.send`, the inbox's RECV at `slots.recv`, its SEND at `inbox_send`). It
  existed before it had a name — imed's settingsd and statefsd legs were hand-built that way so a
  slow statefs PUT can never swallow a settings reply — and modelling it as named slots instead
  would have hidden two provisioned edges from `REQUIRED_ROUTES` and the policy coverage test.
- **Amendment 2026-09-11 (P4f-1b).** A service that RUNS before init wires it allocates its own
  capabilities (VMOs, endpoints) at the lowest free slots, so a late pin can find its declared slot
  occupied. The kernel refuses that (`set_if_empty`) and the pin fails loudly — observed for
  virtioblkd, whose virtqueue VMOs sat in the reply-inbox slots it never used. Invariant: every
  capability init grants an early-running service is pinned before the service runs, or declared
  above the range its own allocations reach; the stage fence (§3) must make "resumed" imply "wired".
- **Amendment 2026-09-11 (P4f-6, closure — implementation evidence).** "Every `new_with_slots(…)`
  is deleted" overstates it: services still build clients from slot numbers — but only from
  declared constants. The rule the gate enforces is sharper, and absolute since P4f-6 (the
  ratchet baseline is gone, 191 → 0): outside `nexus-service-topology` (the slots init
  provisions) and `nexus-abi` (the slots the kernel installs itself — a task's bootstrap
  endpoint, init's endpoint factory — mirrored there like the syscall numbers) no source carries a
  capability slot as a literal: no `const`/`let` slot literal, no literal `new_with_slots`
  argument, no literal `cap_transfer_to_slot` pin; the scanner proves itself on fixtures on every
  run. The control channel stopped being a parameter: `route_with_nonce_budgeted` reads
  `CTRL_SLOTS`. `test_reject_service_missing_from_specs` and `test_reject_clone_leak` landed as
  promised; the latter encodes that a transfer DUPLICATES a capability — init had taken 19 clones
  it never needed. The closure scan also surfaced a route that existed only in its consumer:
  statefsd sent its audit trail to a literal slot 8 that nobody provisioned; the leg is declared now.
- **Amendment 2026-09-11 (P4f-5, implementation evidence).** A consumer that runs before wiring
  receives its STATIC capabilities before it first runs: the proof harness (wave 1) is provisioned
  right after the server-pair distribution, by the same function that serves the generic arm
  (`declared_routes.rs`). Dynamic grants stay dynamic and in-band — `@mint-pair`, and a restarted
  service re-resolved by name (ADR-0057): the slot number travels in the route answer, so it is not
  topology. Routing proofs check the SSOT: the harness compares init's routing answer with the
  declaration (`route_matches`, `test_reject_route_answer_diverging_from_declaration`); its
  `SELFTEST: ipc routing <svc> ok` markers used to follow a hardcoded slot table and proved only
  that a client object could be constructed.
- **Amendment 2026-09-11 (P4f-4, implementation evidence).** A route's delivery kind follows the
  TARGET's reply discipline, not the endpoints init happens to mint. netstackd answers every RPC on
  the caller's CAP_MOVE reply cap and nowhere else, so dsoftbusd's netstackd leg is `ReplyInbox`; the
  per-client netstackd "response endpoint" init minted for it never carried a byte and is deleted
  (the selftest's twin goes with P4f-5). The generic arm resolves a `ReplyInbox` target through the
  minted-pair table (`Endpoints::server_pair`) instead of a hand-kept `ServiceId` → capability match
  that silently skipped any target nobody had added. With the last unmigrated spec declared, the
  order-based pre-grant branch is deleted and `test_reject_partial_slot_declaration` no longer
  exempts unmigrated specs. Witness markers name routes, not slot numbers:
  `init: netstackd policy slots 7/8/9` became `init: netstackd route->policyd ok`.
- **Amendment 2026-09-10 (P4e-2).** A service's server pair is not necessarily handed out by
  its wiring arm: the task-#123 pre-grant pass (`distribute_server_pair_for`) runs first for
  every service with a pre-minted pair, and the arm's own branch is a fallback that normally
  never runs. The pin belongs where the capability is actually transferred, so that pass is
  declaration-driven too — and it never falls back to an order-based transfer when a declared
  pin fails: a failed pin leaves the service unwired with a witness (`init: FAIL declared
  slot …`), which is a dead route you can see, not a service listening on a slot nobody knows.

### 5. Handoff contract v2 — windowd ↔ gpud (normative wire)

`nexus-display-proto` control frames (`[op:u8, payload…]`, integers little-endian):

| Frame | Bytes |
|---|---|
| attach request | `[OP_SET_FRAMEBUFFER_VMO=3, handoff_id:u32]` + CAP_MOVE of the framebuffer VMO (unchanged) |
| **attach ack v2** | `[status:u8, handoff_id:u32, seq:u32, mode_w:u16, mode_h:u16, content_x:u16, content_y:u16, content_w:u16, content_h:u16]` (21 bytes; `status` = `STATUS_OK/MALFORMED/DEVICE_ERROR`) |
| present | `[OP_PRESENT_DAMAGE=4, seq:u32, CommittedBuffer…]` — `seq` strictly increasing per windowd instance, window ≤ 64 outstanding |
| **present ack v2** | `[status:u8, seq:u32]` — echoes the presented `seq`; an unknown or already-acked `seq` at windowd is `windowd: FAIL present ack seq=<n> unexpected` (never forgotten, never "stalled") |
| **reveal** | `[OP_REVEAL=13]` from windowd after wallpaper + cursor upload + first present; gpud latches `reveal_requested` and answers `[STATUS_REVEALED=3, seq:u32]` after the first presented frame at or after that seq |

Rules:

- `OP_GET_DISPLAY_MODE = 11` and `decode_display_mode_reply` are **retired** (P6). The display
  mode has one source, `nexus_abi::boot_display_mode()` (RFC-0074 SSOT), read by gpud, windowd
  and inputd; a device whose capability disagrees is `gpud: FAIL display mode <cfg> vs device
  <cap>` — no default `(1280, 800)` anywhere.
- Reveal is a handshake, not a heuristic: `REVEAL_FALLBACK_NS`, `REVEAL_HARD_CAP_NS`, the
  3-pixel `plane0_has_content` probe and the three `gpud: desktop reveal (…)` variants are
  deleted; one marker `gpud: desktop reveal (handshake seq=<n>)`. Never-black (ADR-0041) holds
  by construction: the splash stays until `STATUS_REVEALED`; a dead windowd is a supervision
  restart, not a black screen.
- `display: first scanout ok` is emitted on `STATUS_REVEALED` (a pixel-backed event), then
  `windowd: present visible ok`; `windowd: fb handoff to gpud ok` stays on the attach ack.
- Present acks are sequence-tracked; `PRESENT_ACK_LEASE_NS`, `LAST_ACK_NS`, `LEASE_REPORTED`,
  the STALL recovery and `FIRST_HANDOFF_DEADLINE_NS` are deleted; the attach stays blocking
  (gpud is ready by construction after §2/§3; a dead peer is a kernel error per ADR-0057).
- gpud's readback proof reads from a non-scanout probe render target (`gl_probe.rs`), never the
  scanout; the same primitive is the ONE readback authority later consumers (screen capture,
  TASK-0068) build on.

Gate: `test_reject_present_ack_without_seq`, `test_reject_attach_ack_without_mode`,
`test_reject_reveal_without_handshake`, `test_reject_unknown_ack_seq`; structure gate: no
`OP_GET_DISPLAY_MODE`, no `1280, 800` literal, no `DISPLAY_MODE_RETRY` in windowd/inputd; no
`nsec()` in gpud's reveal path.

### 6. Display truth as a gate (normative, delivered P0)

The `visible` lane (virgl + egl-headless + VNC) takes a splash snapshot at `gpud: scanout ok`
and a desktop snapshot after `systemui: first frame visible`; the desktop must be non-black AND
differ from the splash (`tools/pixel_proof_judge.py`); under `GPU_MODE=virgl` the booted gpud
must announce `gpud: features=os-lite,virgl` and `gpud: cpu fallback` is a contract violation.
The app-host mount line `APPHOST: mounted hash=` is required in the same block (TASK-0076B
closure). `just test-all` runs the lane.

### Phases / milestones (contract-level)

- **Phase 2** `@ready` + honest `init: up` (§2) — proof: `ci-os-smp1`, `ci-os-reset`, all `ota-*`, `visible`, ingress lanes; `check-init-sync.sh`.
- **Phase 3** routing v2 (§1) — proof: host `test_reject_*`, the same lanes; `init_caps N/N` tally; `!route-deny` watch.
- **Phase 4** topology crate (§4), six atomic sub-packages — proof per consumer: host tests + the consumer's lanes; `check-slot-ssot.sh`.
- **Phase 5** stage fence (§3) — proof: init host tests, `ci-os-smp1` (`stage:` order), `ci-os-reset`, `ci-os-ota-fallback`, `visible`, `display-gpu`; kernel selftest lane if the rights mask lands.
- **Phase 6** handoff v2 (§5) — proof: gpud/windowd/display-proto host tests, chain simulations byte-identical, `visible`, `display-gpu`, `ci-os-smp1`, input lanes.
- **Phases 7–9** polls deleted, closure docs/baselines/gates, 8/8 `just start`-env boots with pixel proof + `just test-all`.

## Security considerations

- **Threat model**: a child forging `@ready` for another service or signalling a stage (spoofed
  readiness); a stale/foreign route reply consumed by the wrong waiter (confused deputy —
  observed in TASK-0321 P5); policy unreachable at route time (privilege by outage); a
  compromised windowd claiming reveal.
- **Mitigations**: `@ready`/`@stage` bound to the spawned pid + `sender_service_id`; nonce
  mandatory and echoed; parked replies answered once; fail-closed policy check; the fence cap is
  WAIT-only; reveal is gpud's latch answered only after a real present.
- **Open risks**: the kernel rights mask for the fence cap (ADR-0062 scopes it); the liveness
  witness for silent stalls (§3) is a kernel selftest addition.

## Failure model (normative)

- Route to an unknown service ⇒ `NOT_FOUND`; to a dead supervised service ⇒ `STALE` (re-ask after
  restart); to a not-yet-ready service ⇒ parked (no reply until ready; overflow ⇒ `DENIED` +
  loud marker); policy deny/unavailable ⇒ `DENIED` + witness.
- Attach to a device whose mode disagrees with the boot mode ⇒ gpud `FAIL` + service exit
  (supervision restart; splash stays).
- Present ack with unknown seq ⇒ windowd `FAIL` marker (lane fails), never silent.
- No silent fallback exists in this contract; every deleted fallback is named in the phase that
  deletes it and guarded by a structure gate.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-init && cargo test -p nexus-service-topology && cargo test -p nexus-display-proto && cargo test -p gpud && cargo test -p windowd
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just ci-os-smp1 && just ci-os-visible && just ci-os-reset && just ci-os-display-gpu-pci
```

### Deterministic markers (three-way SSOT: `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt`, proof manifest)

- kept, re-sourced: `init: up <svc>` (after `<svc>: ready`), `stage: display-ready`, `stage: session-start`, `display: first scanout ok` (on `STATUS_REVEALED`), `gpud: probe sample ok` (P6-d: read through the probe RT, never the scanout), `SELFTEST: display nonblack ok`
- added: `stage: shell-visible`, `windowd: reveal requested`, `gpud: desktop reveal (handshake seq=<n>)`, `KSELFTEST: fence transfer ok` (if the kernel mask lands), `KSELFTEST: liveness snapshot …`
- removed: `gpud: desktop reveal (TIME CAP …)`, `gpud: desktop reveal (plane0 ready, cursor slow)`, `windowd: FAIL present-ack lease expired`, `windowd: STALL present stuck`, windowd/inputd display-mode query lines

## Alternatives considered

- Keep optional nonces and alias guards (rejected: the confused-deputy class stays; TASK-0321 P5 proved it bites silently).
- Time-capped reveal with a pixel probe (rejected: correctness by timer; black-or-not depends on host load).
- `init: up` from resume (rejected: RFC-0013 A1 already forbids reading it as ready; the honest version costs one verb).
- Per-service fences (rejected: one timeline fence with monotone values is sufficient and cheaper; ADR-0052 already provides it).
- Keep positional slots with a lint (rejected: two sources of truth; the crate makes the lint unnecessary).

## Open questions

- Fence-capability rights mask in the kernel cap transfer — decided in ADR-0062 when P5 measures whether `cap_transfer_to_slot` can restrict rights today (owner @kernel-team, TASK-0324 P5).

---

## Implementation Checklist

- [x] **Phase 0**: build truth + visible pixel lane — proof: `just ci-os-visible` (TASK-0324 P0, 2026-09-09)
- [x] **Phase 1**: this contract + ADR-0062 — proof: `just check`
- [x] **Phase 2**: `@ready` + honest `init: up` — `nexus_service_entry::ready`, `ready_table.rs`, responder arm, `check-init-sync.sh` in `just check`; proven 2026-09-09 by `just test-all` (8 lanes green)
- [x] **Phase 3**: routing v2 — nonce mandatory, `route_park.rs`, one reply path, fail-closed policy, `query_route` + alias guards deleted; proven 2026-09-10 by `just test-all` (8 lanes, park observed 3× per lane)
- [x] **Phase 4**: topology crate, bespoke arms deleted — proof: `check-slot-ssot.sh` (absolute since P4f-6) + per-consumer lanes
- [ ] **Phase 5**: stage fence — proof: init tests + `stage:` order in `ci-os-smp1`
- [ ] **Phase 6**: handoff v2 — proof: display lanes + chain simulations
- [ ] **Phase 7–9**: polls deleted, closure, 8/8 boots
- [ ] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
