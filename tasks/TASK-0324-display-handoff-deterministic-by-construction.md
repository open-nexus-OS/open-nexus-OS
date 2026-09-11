---
title: TASK-0324 Display handoff deterministic by construction — build truth, pixel proof, one wiring structure, readiness barriers, handoff contract v2
status: In Progress
owner: @runtime @gpu @ui
created: 2026-09-09
depends-on:
  - TASK-0321 # system volume (gpud/windowd/inputd/hidrawd boot from the volume)
follow-up-tasks: []
links:
  - Plan (approved 2026-09-09): the session plan file; mirrored in the packages below
  - RFC: docs/rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md (P1)
  - ADR: docs/adr/0062-boot-stage-fence-and-readiness-barriers.md (P1)
  - Markers: docs/testing/os-markers.md "Display truth"
  - Related: RFC-0069 (boot stages, §4 Stage field), ADR-0041 (never-black reveal), ADR-0050 (display mode authority), ADR-0057 (STATUS_STALE), RFC-0068 (UART folding)
---

# TASK-0324: Display handoff deterministic by construction

## Why

`just start` showed a BLACK window with every UART marker green (2026-09-09).
Three layers, all verified in the tree:

1. **Build truth broken.** `scripts/build.sh` wired the `virgl` feature only into the
   embedded init-lite loop; gpud had moved onto the system volume (TASK-0321 P4b) and
   every bundle was built with a literal `--features os-lite`. A 2D gpud ran on a
   `virtio-gpu-gl` device. Embedded and bundle builds also wrote the SAME artifact
   with different features (last build wins).
2. **The 2D plane-row scanout is black on every GL display backend** (they blit the
   scanout texture from row 0; gpud's `SET_SCANOUT{y=1600}` is ignored). Only the GL
   render target path is correct on GL devices.
3. **The proof chain was not display truth.** `display: first scanout ok` is windowd's
   send, not a pixel; the harness guards required a marker the boot splash emits; no
   lane checked pixels since 2026-07.

Underneath: the display chain (gpud, windowd, inputd, hidrawd, touchd) sits outside
init's declarative arm — positional slots hardcoded on both ends, nonce-less routing with
alias guards + wired-slot fallbacks, `init: up` printed for suspended processes, `yield_()`
as a barrier, a reveal decided by a 3-pixel probe and time caps. Under SMP that is a
race by construction. The user's rules for this task: **no dual structure** (every new
mechanism replaces and deletes the old one; gates prevent building into the old
structure; scope = ALL bespoke arms), and **nothing recently built may break** (volume
boot, OTA lanes incl. `bundle reused`, reliability spine, ingress/egress, folding).

## Packages

| # | Package | Content | Status |
|---|---------|---------|--------|
| P0 | Build truth + honest GL path + `visible` pixel lane | `[package.metadata.nexus-service]` = SSOT for features (`feature_profiles` keyed on env); `discover-services.sh` resolves for embedded + bundles + execd payloads; keyed ELF `build/services/<svc>/<key>/`; `check-bundle-provenance.sh`, `check-build-truth.sh` (in `just check`); `gpud: features=…` first raw line; `backend/scanout_policy.rs` (GL device → GL RT or FATAL; 2D retry deleted); `[profile.visible]` + `just ci-os-visible` in `test-all` (VNC snapshots splash/desktop, `pixel_proof_judge.py`); `just start` exposes VNC 5979; harness requires `gpud: features=os-lite,virgl` under virgl, `gpud: FAIL` fails lanes. Deleted: virgl special block, `*_CARGO_FLAGS` (build.sh + harness netstackd), stack-pages `case`, metricsd stack override, `gpud: gl scanout fallback 2d`. | Delivered 2026-09-09 (`just check` + `test-all` stages green: check/diag/dep-gate/host/e2e/miri/kernel; lanes smp1 (rerun after a pre-existing `statefs persist` deny flake), visible ×4 pixel-proof ok, reset, ota-flip/bundle/resume/delta/backstops; 8/8 `just start`-env boots pixel ok over harts 0-3) |
| P1 | RFC-0093 + ADR-0062 + ledger/board | routing v2 (nonce, parked replies until `@ready`, fail-closed), `@ready`/`@stage`, `ServiceSpec.stage` + fence, declared slot map for services AND app children, handoff v2 (attach ack {seq, mode, content_rect}, `OP_REVEAL`/`STATUS_REVEALED`, seq acks), display mode from `boot_display_mode()`, pixel proof as display gate | Delivered 2026-09-09 (paper: RFC-0093 + ADR-0062 written, indexed; ledger/board/order updated; `just check`) |
| P2 | `@ready` + `nexus_service_entry::ready()` + honest `init: up` | fleet-wide; `ready_table.rs`; delete `init: up` in orchestrator/volume_spawn; docs/manifest | In Progress 2026-09-09 — code + host tests + gate landed (29 ready sites migrated, `init: up` only from the responder `@ready` arm, `check-init-sync.sh` in `just check`, ladder `init: up` presence-only); Done 2026-09-09 — `just test-all` green (exit=0): 8 lanes (smp1, visible, reset, ota, ota-bundle/-resume/-delta, ota-backstops), 0 announce failures fleet-wide, 29 honest `init: up` in the visible lane, pixel proof 40.1% non-black |
| P3 | Routing v2 | delete v1 `query_route`; nonce mandatory; `route_park.rs`; bounded-blocking replies; fail-closed policy; delete windowd alias guards | Done 2026-09-10 — `just test-all` green (exit=0, 8 lanes); ⭐ park scope CORRECTED against the RFC seed: an ask parks on an unresolvable (stale/restarting) target, never on the target's readiness — the latter is cyclic (`bundlemgrd` → `metricsd`, which boots from the volume bundlemgrd serves), readiness ordering is P5's fence; RFC-0093 §1 amended with that evidence. evidence across smp1/visible/reset/ota-flip: 28/29/65/53 honest `init: up`, ZERO malformed asks, ZERO dropped replies, ZERO policy-unreachable events, and the park proven in every lane — `init: route resumed svc=selftest-client -> pinched` exactly 3×, one per restart cycle of the supervision probe |
| P4 | `nexus-service-topology` = one slot SSOT | crate move + slots; declared arm; per-consumer atomic sub-packages P4a windowd, P4b inputd, P4c gpud, P4d hidrawd/touchd, P4e execd + app children (`nexus-sdk-routes` view), P4f policyd/netstackd/bootctld/keystored/updated/dsoftbusd/bundlemgrd/metricsd/imed/selftest-client; delete `is_bespoke_wired` + all arms + slot-order comments; `check-slot-ssot.sh` | P4-base delivered 2026-09-10 (crate created from init's declarations + slot fields, init re-exports, sdk-routes cross-validated, `check-slot-ssot.sh` ratchet in `just check`: 191 positional declarations in 65 files, may only shrink); `just test-all` green 2026-09-10 over 8 lanes — the move is behaviour-neutral by construction (init re-exports, no slot values changed yet); **P4a windowd delivered 2026-09-10**: windowd's routes completed in the declaration (gpud/abilitymgr/imed were provisioned by init but never declared), ALL its slots declared (`slots::windowd`), init PINS them via `cap_transfer_to_slot` (`bootstrap/declared_slots.rs`), windowd reads the same constants — its `GPUD_WIRED_*`, `new_with_slots(3,4)`, `WATCH_*` and `CTRL_*` copies are gone, and with them the "do not reorder or gpud shifts to 8/9" comment chain. Ratchet 191→181 declarations / 65→60 files. ⭐ The P3 witness paid for itself in the first lane run: `statefsd: FAIL policy unreachable cap=statefs.read` named what used to be a silent `access denied`, and the log proved starvation rather than a dead peer (policyd printed its next line immediately after the timeout and every following statefs op succeeded) — the 500 ms wait was a scheduling guess, now a 2 s LIVENESS bound (a supervised peer silent that long is a real outage; ordering the UI chain against control-plane services is P5's fence). Proof: `just test-all` green 2026-09-10 (8 lanes) — zero `FAIL declared slot`, zero `FAIL policy unreachable`, and the display handoff resolves over the declared route (`windowd: gpud route connected` → `fb handoff to gpud ok`, not the wired fallback). **P4b inputd delivered 2026-09-10**: inputd had NO declaration at all — it was wired entirely from a bespoke arm whose comments called the windowd leg's slot numbers "a boot contract". It now has a spec (routes to windowd + imed, both `SharedResponse`), declared slots (`slots::inputd`: server 3/4, windowd 5/6, imed 7/8, settings 0x20, watch 0x21/0x22), init pins them, and inputd reads the same constants — its three `new_with_slots(5, 6)` copies, the `new_with_slots(3, 4)` literal and the three `0x2x` consts are gone, as are init's now-dead `INPUTD_WATCH_*`. Ratchet 181→176 declarations / 60→59 files. Proof: `just test-all` green 2026-09-10 (8 lanes) — zero pin failures, and the input chain runs over the declared slots (`inputd: chain I5 delivered to windowd` → `windowd: chain I6 input recv`). **P4c gpud + the fleet-wide MMIO slot delivered 2026-09-10**: gpud declared (pure server, `slots::gpud::SERVER` 3/4) and pinned by init; its `GPUD_RECV/SEND_SLOT` read the declaration, `GPU_IRQ_NOTIFY_SLOT` now names `CTRL_SLOTS.recv` (the reuse it always was), and `DEVICE_MMIO_SLOT` became ONE fleet-wide constant — the number 48 lived in seven places (init + virtioblkd + rngd + timed + gpud + two selftest probes) and now lives once. ⭐ With gpud migrated, init's order-based `try_transfer` helper lost its last caller and was DELETED: every capability init hands a service now lands in a declared slot. Ratchet 176→166 declarations / 59→56 files. Proof: `just test-all` green 2026-09-10 (8 lanes) — zero pin failures, display path intact (`gpud: features=os-lite,virgl` → `fb handoff to gpud ok` → pixel proof). **P4d hidrawd delivered 2026-09-10**: hidrawd declared (pure producer — pushes normalized HID events to inputd, exposes no endpoint), its route pinned, its `CTRL_*` copies and the literal `[50, 51, 52]` input-window array replaced by `INPUT_MMIO_SLOTS` (init held the same block as a base constant). touchd needed no migration: it holds no capability beyond the control channel — recorded rather than invented. Ratchet 166→163 declarations / 56→54 files. Proof: `just test-all` green 2026-09-10 (8 lanes) — zero pin failures, HID chain intact (`hidrawd: virtio-input … ready` → `inputd: chain I5 delivered to windowd`). **P4e (app-child half) delivered 2026-09-10**: the SPAWNED-APP-CHILD capability table is declared once (`slots::app_child`: windowd 5/6, payload VMO 7, events RECV 8, reply inbox 9/10, svc base 11, events SEND 14, atlas VMO 19, minidump statefs 7/8). It lived THREE times — execd's grant constants, the app-host's "fixed constants" and `nexus-sdk-routes` — with each side's comment pointing at the other side. execd, app-host (incl. the three bare `WINDOWD_SEND_SLOT = 5` literals in the effect modules) and sdk-routes now read the declaration. Ratchet 163→146 declarations / 54→49 files. Proof: `just test-all` green 2026-09-10 (8 lanes) — app launch and mount unaffected (`APPHOST: mounted hash=`, pixel proof). **P4e-2 delivered 2026-09-10**: execd's OWN table is declared — it had no `ServiceSpec` at all, so every slot it owns came out of TRANSFER ORDER: server 3/4, reply inbox 5/6, logd 7, the delegated windowd route 8/9, bundlemgrd 10, the recv-wake probe endpoints 11-14 and the twelve named routes it resolves for spawned app-hosts (15-24). The order WAS the contract, written in comments ("keep this block FIRST: the probe slots are POSITIONAL", "ARM END on purpose: transfers here must never shift earlier positional slots") and backed by `probe_dump_cap_slots()` in execd — a diagnostic whose only job was to name a drift after it had already produced a dead handshake. init pins all of them; execd, `child_grants` and the probe binary read the same constants. ⭐ The pre-grant pass (`distribute_server_pair_for`) — which is what actually hands execd its server pair — is now declaration-driven for every migrated service and never falls back to an order-based transfer on a failed pin (a witness instead of a service listening on an unknown slot). `Execd → Logd|Timed|imed-osk|Windowd` were provisioned but never declared, so the host cross-checks could not see them; they are in `REQUIRED_ROUTES` now. Structure: slots → `slots.rs`, execd's arm + named routes → `bootstrap/execd_wiring.rs` (`wiring.rs` 1784 → 1586 LOC). Ratchet 146→134 declarations / 49→46 files. Proof: `just check` + `just test-host` green; all 8 lanes green 2026-09-10/11 (smp1, visible pixel 40.09 % / diff 22.18, reset, ota-flip, ota-bundle, -resume, -delta, -backstops) with zero `init: FAIL declared slot`, `execd: recv-wake probe ok` + `SELFTEST: exec child blocking recv wake ok` (probe endpoints 11-14 and child slots 5/6 through the declaration), `APPHOST: mounted hash=` (app launch via the delegated windowd route + bundlemgrd payload), `execd: app route granted svc=session|time`. HONEST NOTE: the lanes did not run as one `test-all` — the host's memory watchdog SIGTERMed the aggregate run four times (desktop memory pressure, not the lanes: a lane measured 726-845 MB), so they were completed individually; that failure mode is what TASK-0325 turned into a witnessed verdict. **Then proven in ONE run:** cold `make clean` + `just test-all` green 2026-09-11 (rc=0, all 8 lanes, visible pixel 40.09 % / diff 22.18, zero `FAIL declared slot`, `execd: recv-wake probe ok`, `APPHOST: mounted hash=`). P4f open |
| P5 | Stage fence | `stage_fence.rs`; `ServiceSpec.stage`; entry hook waits; delete `yield_()` sync + `resume_drivers` order; `check-init-sync.sh` | Draft |
| P6 | Handoff contract v2 (windowd↔gpud) | explicit reveal (delete time caps/3-pixel probe), seq acks (delete lease/stall recovery), kernel display mode everywhere (delete windowd/inputd mode polls), readback off the scanout RT, `display: first scanout ok` on `STATUS_REVEALED` | Draft |
| P7 | Consumer polls deleted | windowd session probe/greeter watch/cursor wait; app-host content-rect re-drive | Draft |
| P8 | Closure docs/baselines/gates | LOC baseline, docs (RFC-0069 §4 implemented, RFC-0013, ADR-0041/0050), CI parity | Draft |
| P9 | 8/8 visible boots + test-all + progress table | | Draft |

**P4f recut (2026-09-11, measured against the tree before any code):** the seed listed ten
services; the slot ratchet and a scan of every capability transfer in init show the real scope.
**23 consumers**: 15 on the GENERIC arm (abilitymgr, bootctld, imed, ingressd, logd, packagefsd,
pinched, rngd, samgrd, sessiond, settingsd, statefsd, timed, vfsd, virtioblkd) and 8 bespoke
(policyd, keystored, updated, bundlemgrd, netstackd, dsoftbusd, metricsd, selftest-client). And a
**blind spot of the ratchet**: ~40 init call sites already pin into LITERAL numbers
(`cap_transfer_to_slot(pid, cap, R, 0x07)` — netstackd, policyd, dsoftbusd, bootctld, imed, blk
plane, metricsd…). They are "pinned" but not declared: the same number in init and in the service,
agreeing by hand — exactly the dual contract P4 exists to remove — and `check-slot-ssot.sh` counts
only `const …SLOT` declarations, so it never saw them. Sub-packages (each atomic, each proven by
`just test-all`; the generic arm's order-based branch dies with P4f-1, the literal pins with the
consumer that owns them):

| # | Consumers | Deletes |
|---|-----------|---------|
| P4f-1 | generic arm + its 15 services | order-based transfers in the generic arm, `provision_server_endpoint`'s order-based pair, the fresh-endpoint branch of `distribute_server_pair_for`, literal pins for bootctld/imed/blk plane/statefsd; the services' CTRL 1/2 + server 3/4 + route copies |
| P4f-2 | policyd, keystored | `core_plane.rs`/`policyd_slots.rs` literal pins, both bespoke arms' transfer order |
| P4f-3 | updated, bundlemgrd | the update plane's arms + `updated`'s four client files' slot copies |
| P4f-4 | netstackd, dsoftbusd, metricsd | literal pins 0x03-0x09 / 0x21-0x22 in `wiring.rs`, the facade's copies |
| P4f-5 | selftest-client | `route_with_retry`'s hardcoded eight-service slot table + its polling route ask, every probe's slot copy |
| P4f-6 | closure | `check-slot-ssot.sh` also rejects literal slot numbers in `cap_transfer_to_slot`/`new_with_slots` outside the topology; baseline → 0 |

**P4f-1a delivered 2026-09-11 (architecture review first — `.claude/skills/architecture-review`):**
twelve generic-arm services declared completely (abilitymgr, ingressd, logd, packagefsd, pinched,
rngd, samgrd, sessiond, settingsd, statefsd, timed, vfsd) with their EFFECTIVE numbers, so the move
is behaviour-neutral by construction; `SERVER_SLOTS` (4/3) is one fleet convention instead of a
`SlotPair::new(4, 3)` per module and a literal `new_with_slots(3, 4)` per service. The generic arm
PINS every leg of a declared service (server fallback, reply inbox, SharedResponse + ReplyInbox
routes; `provision_server_endpoint` and the distribute fresh-endpoint branch too) through
`declared_slots::grant_*`, which decide by DECLARATION, never by pin success; the order-based
branch survives only for imed/bootctld/virtioblkd and dies in P4f-1b. abilitymgr→execd was declared
`ReplyInbox` but provisioned as SharedResponse by a special block — kind corrected, block deleted,
the generic branch prints the same `init: abilitymgr route->execd ok`. pinched's respawn pins
CTRL_SLOTS + the declared server pair instead of transferring by order and checking 3/4 afterwards;
`spawn.rs`'s CTRL literals and the orchestrator's never-firing post-check are gone. Services:
literal 3/4 fallbacks, CTRL copies, ingressd's slot module and statefsd's
`check_cap_on(0x07, 0x06, 0x05)` read the declaration. New host test
`test_reject_slot_in_reserved_range` (proven to fail on a declaration at MMIO slot 48). Ratchet
130/45 → 93/33. **Review decisions:** (1) the self-route ask for a service's OWN server stays — it is
an implicit readiness barrier (the responder answers only after wiring), removing it is P5's; only
its literal fallback went. (2) The eight hand-copied `route_blocking` helpers were NOT merged
(different budgets; P7). **Findings:** (a) the kernel's `CopyToSlot` is `set_if_empty` — a pin into
an occupied slot errors, it never overwrites, so a wrong declaration fails loudly instead of
displacing a device capability; the same code shows a transfer DUPLICATES (derived rights), several
init comments claiming "a transfer MOVES the cap" are wrong. (b) logd, pinched, rngd, sessiond,
settingsd and statefsd are missing from `config/os-services.txt`, so `just diag-os-strict` never
compiles them under the OS cfg — the hard warning gate has a hole (a deliberate `compile_error!`
proved rngd's OS module is compiled by an explicit check). (c) `init: route fallback` for
virtioblkd/bundlemgrd/logd/samgrd is pre-existing in every smp1 run of the day (core-plane route
budget vs responder start — P5). Smoke: smp1 green, 0× `FAIL declared slot`, pinched respawn 3× over
the declared pair, `SELFTEST: service restart ok`, `statefs persist ok`, `enc roundtrip ok`.
**Proof 2026-09-11:** `just test-all` green through its gates (check, diag, host, e2e, miri, kernel)
and the smp1 + visible lanes (pixel 40.09 % / diff 26.03); the host memory watchdog then SIGTERMed the
reset lane — and for the first time the kill was WITNESSED (`qemu-test: lane terminated externally
(signal TERM) after 129s`, lane peak 737 MiB, zero envelope events: TASK-0325 paying off). The
remaining lanes ran one by one with a retry ONLY on a witnessed external kill: reset, ota-flip,
ota-bundle, -resume, -delta, ota-backstops (tamper, downgrade, fallback) all green on the first
attempt, 0× `FAIL declared slot` in every lane, lane peaks 740-1,113 MiB.

**P4f-2 delivered 2026-09-11:** policyd and keystored leave their bespoke arms
for the generic arm (policyd's 62-line arm with a never-matching `else` branch that would have
spread its inbox by transfer order, keystored's 192-line arm of transfer tracing, and
`policyd_slots.rs` are deleted; the generic bridge gained the rngd target). policyd's server pair and
init's route-check/exec-check channels are pinned in the CORE plane, before `resume_plane`
(`NamedSlot::PolicyRouteCheck*/PolicyExecCheck*`), its audit inbox 0x9/0xA and logd leg 0xB by the
generic arm; keystored's inbox 5/6 + statefsd 7, logd 8, policyd 9, rngd 0xA are declared. The policy
coverage test exempts the policy authority by name (a policyd route ask would ask policyd — adding
`ipc.core` would be a privilege that gates nothing). Structure: `SERVICE_SPECS` is a list of named
per-service consts in `specs_{app,storage,security,ui}.rs`; keystored's policy check moved to
`policy_os.rs`. Ratchet 72/27 → 57/22. **Findings:** (1) ⭐ `Endpoints::server_pair` had NO policyd
entry, so every leg that looked policyd up there was silently skipped — execd's crash attach-level
route (`crash.attach.*`, TASK-0051B) had NEVER been provisioned: every lane printed
`init: execd route->…` for eight targets and none for policyd; execd's delegated check resolved to
`Unreachable` → deny → `AttachLevel::None`, although `policies/base.toml` grants execd
`crash.attach.stack`. Fixed; the smoke lane prints `init: execd route->policyd ok` for the first
time. (2) `SELFTEST: crash redaction ok` stayed green through it: the probe compares
`has_stack == source_had_stack` for the demo.minidump child, whose capture evidently carries no stack
preview — so the probe passes vacuously and never proves the stack-only path (no marker prints the
resolved level). Recorded for TASK-0051B follow-up. (3) keystored's policyd leg sat at 9 only
because the OPTIONAL logd leg was transferred first; an image without logd would have sent its policy
checks to rngd's slot. (4) More ratchet blind spots: slots as `let` literals
(`let rng_send_slot = 0x0a;`, `let ctl_route_recv_slot = 5;`) — for P4f-6's gate. **Proof:** `just check` green; `just test-all` green through its gates + smp1 + visible, then the
host memory watchdog killed the reset lane (witnessed) and afterwards the lane runner task itself; with
the user's go-ahead the runner ran detached from the agent task (the lanes stay inside their 4G/8G
envelope): reset, ota-flip, ota-bundle, -resume, -delta, ota-backstops (tamper, downgrade, fallback)
all green on the first attempt. 0× `FAIL declared slot` in every lane; smoke evidence
`init: execd route->policyd ok` (first time ever), 27× `policyd: audit emit ok`,
`SELFTEST: keystored v1 ok`, `device key persist ok`, policy allow/deny/audit/spoof all ok.

**P4f-1b delivered 2026-09-11:** new route kind `PrivateInbox { inbox_send }` (imed's settingsd
8/9/10 and statefsd 0x0B/0x0C/0x0D legs are declared routes the generic arm provisions — the
hand-built legs with pinned literals and a never-closed `cap_clone` each are deleted); imed's OSK RECV
is `NamedSlot::OskServerRecv` (5) and its windowd route (6/7) is pinned; bootctld's
`provision_bootctld_fixed_slots` and its bespoke arm are deleted — inbox 5/6, statefsd 7 and policyd 8
are declared and pinned by the generic arm, still before the boot-attempt handshake; the block plane
is `BLK_PLANE_REQ_SLOT`/`BLK_PLANE_REPLY` in the topology and `storage::blockproto` re-exports it;
virtioblkd's IRQ notify moved 0xF1 → 0xF3 (off the clients' reply RECV number) and it no longer
receives the unused reply inbox; the generic arm pins directly (the declaration-or-order helper
survives only for the core plane's policyd/bundlemgrd); `routes.rs` split out of `specs.rs`; the
generic arm's six announce copies are one helper. Ratchet 93/33 → 72/27. **Proof:** `just test-all`
green in ONE run (exit 0, no watchdog kill): gates + smp1, visible (pixel 40.09 %), reset, ota-flip,
ota-bundle, -resume, -delta, ota-backstops — 0× `FAIL declared slot` in every lane, with
`SELFTEST: ime ranking persist ok` (the private statefsd inbox carries real traffic),
`SELFTEST: bootctl persist ok`, `init: health ok (slot a|b)`, `virtioblkd: irq endpoint bound`.

**Findings for P4f-1b (recorded 2026-09-11, before its proof):** (1) ⭐ **A service that runs
before init wires it competes with init's late pins for the low slots.** The first P4f-1b smoke lane
failed loudly with `init: FAIL declared slot reply recv slot=0x5` for virtioblkd: the wave-0 block
driver is resumed early to serve the system volume and allocates its virtqueue VMOs itself
(`vmo_create`), and the kernel hands those out at the lowest free slots — 5, 6 — before the generic
arm pins the reply inbox there. The pin refused the occupied slot (never overwrote), so the design
held; the declaration was wrong because it was DERIVED from transfer order, not measured. Before P4f
the inbox landed wherever order put it, unnoticed — and unused: virtioblkd makes no outbound call and
never read it (two doc comments still claimed it "doubles as the IRQ notify endpoint", stale since
TASK-0315 gave the IRQ its own endpoint). Fix = least privilege: virtioblkd holds no reply inbox.
**Invariant for P4f-2/P4f-3/P5:** every capability init grants an early-running service (core
plane: virtioblkd, policyd, bundlemgrd) must be pinned BEFORE that service runs, or declared above
the range its own runtime allocations reach — and the stage fence must make "resumed" imply
"wired". (2) The kernel's transfer DUPLICATES (`caps.derive`), so the `cap_clone`-then-transfer
pattern in init leaks one slot of init's own table per leg — the clone is never closed. Counted
2026-09-11: execd_wiring (9 legs), updated→policyd, blk_plane's updated vfs leg, the bootctld boot
request, orchestrator's windowd/inputd clones and endpoints' imed-osk clones; imed's two legs are
gone with P4f-1b (they became declared `PrivateInbox` routes without a clone). A one-time ~14 slots
at boot, not per launch — recorded for P4f-6, where init's cap hygiene gets a gate.

**Carried into P4f and P7 (recorded 2026-09-11, cold `make clean` + `test-all`):** the
cold run failed `ota-flip` with `SELFTEST: statefs enc roundtrip FAIL` — NOT a P4e-2 regression
and not a resource kill (lane peak 830 MB, zero envelope events): `rng_salt()` in the selftest
client POLLED for rngd's reply (`recv(NonBlocking)` + `yield_()` against a 500 ms wall clock),
and rngd asks policyd before it answers (`rngd: policy check` printed right after the selftest
gave up). The lane runs without icount, so guest time is host time, and the cold build had the
host busy. Same defect class P3 removed from `nexus_ipc::policyd`. Fixed now for rngd (user
decision): the selftest client's three hand-copied rngd exchanges (two entropy probes + the
salt) are ONE client, `services/rngd.rs`, that WAITS (deadline-bounded blocking recv, nonce
filter, 2 s liveness bound). **Open, by count, so nothing is lost:**
- **P7 — the remaining poll-against-wall-clock sites** (`recv(NonBlocking)` + `yield_()` +
  deadline, 2026-09-11 scan): `source/services/windowd/src/compositor/mod.rs`,
  `source/services/inputd/src/os_lite.rs`, `source/services/updated/src/os_lite.rs`,
  `source/apps/selftest-client/src/os_lite/services/statefs.rs`,
  `source/apps/selftest-client/src/os_lite/probes/ipc_kernel/plumbing.rs`, and the generic
  helper `userspace/nexus-ipc/src/budget.rs` (`retry_ipc_until`, which `reqrep::recv_match_until`
  and further callers sit on — the helper is the real unit of the fix).
- **P4f — the selftest client's routing layer** (`os_lite/ipc/routing.rs::route_with_retry`) is
  itself a bespoke arm: a hardcoded slot table for eight services (bundlemgrd 9/A, updated B/C,
  samgrd D/E, execd F/10, logd 15/16, policyd 7/8, keystored 11/12, statefsd 13/14) plus a
  route-ask loop that polls `QueueEmpty` with `yield_()`. The rngd route (1e/1f) now lives once in
  `services/rngd.rs` and migrates with it.

**Findings for P4e-2 (recorded 2026-09-10):** (1) execd's server pair is not handed out by
its wiring arm at all — the task-#123 pre-grant pass (`distribute_server_pair_for`) gets there
first, and the arm's own branch is a fallback that normally never runs. A consumer migration
that only rewrites the visible arm would have left the real assignment order-based; the pin
belongs where the capability is actually handed over. (2) The `ServiceSpec` for a bespoke-wired
service is safe (windowd/gpud/inputd/hidrawd already have one): the generic provisioning arm is
gated on `is_bespoke_wired`, not on the presence of a spec — the supervision SSOT's warning
("must never grow one") predates that gate. (3) execd holds a windowd route it never uses: it
clones both halves into every app child. Declared as a route with the delegation stated, because
it IS provisioned like one — leaving it undeclared is what let it sit at "whatever slot came
after logd" for a year. (4) Two artifacts of earlier module splits surfaced while editing the
same lines: 21 `init: … route->… ok` diagnostics printed a literal `\n` (`b"…ok\\n"`) instead of
a newline — invisible because those lines only print under the verbose wiring diagnostics — and
`provision_execd_named_routes`'s doc comment plus its `#[allow(clippy::too_many_arguments)]` sat
on `updated_policyd_leg` two functions earlier. Both fixed here.

**Findings for P3 (recorded 2026-09-10):** (1) Parking on readiness is cyclic (see the ledger
row); the honest park criterion is "init cannot resolve it yet", which only its own
bookkeeping decides. (2) Two nonce-correct but hand-copied route helpers remain
(`source/apps/selftest-client/src/os_lite/ipc/routing.rs`,
`source/services/bundlemgrd/src/os_lite.rs`): they ask once with a nonce, so they are
duplication rather than a defect — they resolve slots and therefore belong to P4's topology
crate, and the P3 gate forbids only what P3 deleted. (3) The park is not theory: in the supervision/crash-loop probe the selftest asks for the
killed `pinched`, init holds the ask and answers it once the supervisor re-provisions the
route (`init: route resumed`, 3× in one boot) — the client never re-asks. (4) The `@ready`
verb is one-way and therefore EXEMPT from the nonce requirement (nothing to correlate); the
first lane run failed because the requirement was applied before the verb arm and every
service's announce was refused as malformed. (5) ⭐ The `statefs persist FAIL` flake the P0/P2 runs recorded is a control-plane defect, not
load: `nexus_ipc::policyd::exchange_status_on` POLLED for the reply (NONBLOCK + `yield_()`
against a 500 ms wall clock), so a policyd that was alive but not scheduled inside the budget
was indistinguishable from one that never answered — and statefsd collapsed
`CapDecision::Unreachable` into "not allowed", printing only `access denied`, which reads like
a policy decision. Fixed in this package: the exchange now WAITS (deadline-bounded blocking
recv — the arriving reply wakes it, the deadline only fires if the peer really did not
answer), and statefsd names the outage (`statefsd: FAIL policy unreachable cap=…`) while still
denying. Note the reply inbox is shared across a service's outbound calls (statefsd has three
users: cap check, ABI seam, logd append), which is why the nonce filter must drop foreign
frames and keep waiting. (6) `nexus-service-entry` had to be split
(`os/ready.rs`, `os/alloc_log.rs`) and `responder.rs` shrank 548 → 533 LOC because the
nonce-mandatory rule collapsed six duplicated reply blocks into one helper module
(`bootstrap/route_reply.rs`).

**Findings for P2 (recorded 2026-09-09, smp1 iterations):** (1) `@ready` could not be queued
for virtioblkd/logd: their bespoke `route_*_blocking` probes re-sent the same route ask on
every one of 64 iterations and filled init's control REQ queue (depth 8; even 32 was not
enough) before the responder ever ran — replaced by ONE `route_with_nonce_budgeted` ask
(50 ms budget, deterministic slot fallback unchanged); this is P3's "never re-ask" rule
landing early and the first two bespoke route copies deleted. (2) A bounded-BLOCKING announce
is wrong by construction: init's orchestration waits on the block driver (system volume),
so a driver waiting in `ready()` for init to drain the queue is a circular wait (init fatal
`volume-unavailable`); the announce is a few non-blocking attempts, loud on failure.
(3) Kernel finding for P5 (approval zone, not fixed here): `sys_ipc_send` arms the timer
wakeup for a blocking send BEFORE the first attempt and never disarms it when the attempt
fails immediately (`ipc_msg.rs:230`), so a service that got `NoSuchEndpoint` on a deadline
send is woken spuriously later inside its server recv. (4) Three ladder rungs were
spawn-fakes that no readiness backs: `init: up dsoftbusd` (ready only on the single-VM
session path → moved into the `REQUIRE_DSOFTBUS` block / manifest net phase),
`init: up hidrawd` and `init: up touchd` (device-gated; hidrawd prints `input slot missing`
without input devices, touchd never runs to ready in proof boots — the P5 item) → removed
from every base list, declared for the `full` profile only. (5) The responder's `init: up`
is written atomically (`emit_marker_atomic`); a three-fragment write tore against gpud
(`init: up keystoredgpud: …`).

**Findings for P5 (recorded 2026-09-09, P0):** `touchd: os service payload ready` — a
`full`-ladder marker — is never printed in proof boots (verified: headless full ladder at
4 harts prints it only in a lull right after the Idle-class dsoftbusd, at 1 hart never;
Normal QoS, cpu0 pin or `0b1110`, 8 stack pages make no difference). No lane has
required it since 2026-07. A resumed Normal task that does not run for the whole boot
is a scheduler/readiness determinism defect; P5's stage barriers must make "resumed"
imply "ran to ready" (and a P5 host+QEMU proof must cover it). Also: `stack_pages` in
the manifests had never been honoured before P0.

**Finding for P3 (recorded 2026-09-09, P0 gate run):** `SELFTEST: statefs persist FAIL` in
1 of ~14 smp1 runs (also 2026-09-07): `statefsd: access denied path=/state/selftest/persist`
because `nexus_ipc::policyd::check_cap_on` (fixed slots 7/6/5, bounded wait) treats a missing
policyd reply as `Deny` — fail-closed, but with no witness line and no retry. Routing v2 /
parked replies (P3) must make "no reply" impossible for a live target and name it when it
happens (`statefsd: policy unreachable`), never a silent deny.
Second sample of the same class in the P0 gate (visible lane, 1 of 4 runs): `statefsd: write
budget exceeded (ns=334173500)` → `keystored: persist FAIL step=put err=Corrupted` with the
2026-09-08 witness `keystored: persist put status=0xfd` = the reply frame was UNDECODABLE
(not "no reply"): a slow PUT's answer arrives as a frame the client cannot decode — a stale
or foreign frame on the shared reply slot. **Root-caused and fixed in P0:** keystored built
the statefs client WITHOUT `statefs/os-lite`, i.e. the unfiltered host-style path; the
feature is set and the statefs client now `compile_error!`s on the OS target without it.

**Open evidence item (P5/P9):** one silent system stall in 16 interactive boots (2026-09-09
11:34, first boot after a lane): UART stops at ~3.0 s right after the driver resume
(`hidrawd: irq endpoint bound`, imed verdict), no `gpud: features=` line, no panic, no
watchdog for 22 s. No kernel witness exists for "every hart idle, nothing runnable, tasks
blocked" — P5 must add one (liveness snapshot on a quiet-system timeout) before the
stage barriers can be called deterministic.

Per package: host tests → `just check` → `just test-all` incl. the lanes named in the plan
(blast radius) → docs sweep → commit proposal (user commits).

## Not in this task

Compositor/present pipeline redesign; session UI/OSK/greeter design; network family
(HOLD); QEMU/virglrenderer changes. The 2D device path (`virtio-gpu-device`,
`display-gpu-pci` lane) stays as the ONLY path for non-GL devices.
