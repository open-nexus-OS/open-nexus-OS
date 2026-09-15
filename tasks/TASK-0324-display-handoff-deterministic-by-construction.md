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
| P4 | `nexus-service-topology` = one slot SSOT | crate move + slots; declared arm; per-consumer atomic sub-packages P4a windowd, P4b inputd, P4c gpud, P4d hidrawd/touchd, P4e execd + app children (`nexus-sdk-routes` view), P4f policyd/netstackd/bootctld/keystored/updated/dsoftbusd/bundlemgrd/metricsd/imed/selftest-client; delete `is_bespoke_wired` + all arms + slot-order comments; `check-slot-ssot.sh` | P4-base delivered 2026-09-10 (crate created from init's declarations + slot fields, init re-exports, sdk-routes cross-validated, `check-slot-ssot.sh` ratchet in `just check`: 191 positional declarations in 65 files, may only shrink); `just test-all` green 2026-09-10 over 8 lanes — the move is behaviour-neutral by construction (init re-exports, no slot values changed yet); **P4a windowd delivered 2026-09-10**: windowd's routes completed in the declaration (gpud/abilitymgr/imed were provisioned by init but never declared), ALL its slots declared (`slots::windowd`), init PINS them via `cap_transfer_to_slot` (`bootstrap/declared_slots.rs`), windowd reads the same constants — its `GPUD_WIRED_*`, `new_with_slots(3,4)`, `WATCH_*` and `CTRL_*` copies are gone, and with them the "do not reorder or gpud shifts to 8/9" comment chain. Ratchet 191→181 declarations / 65→60 files. ⭐ The P3 witness paid for itself in the first lane run: `statefsd: FAIL policy unreachable cap=statefs.read` named what used to be a silent `access denied`, and the log proved starvation rather than a dead peer (policyd printed its next line immediately after the timeout and every following statefs op succeeded) — the 500 ms wait was a scheduling guess, now a 2 s LIVENESS bound (a supervised peer silent that long is a real outage; ordering the UI chain against control-plane services is P5's fence). Proof: `just test-all` green 2026-09-10 (8 lanes) — zero `FAIL declared slot`, zero `FAIL policy unreachable`, and the display handoff resolves over the declared route (`windowd: gpud route connected` → `fb handoff to gpud ok`, not the wired fallback). **P4b inputd delivered 2026-09-10**: inputd had NO declaration at all — it was wired entirely from a bespoke arm whose comments called the windowd leg's slot numbers "a boot contract". It now has a spec (routes to windowd + imed, both `SharedResponse`), declared slots (`slots::inputd`: server 3/4, windowd 5/6, imed 7/8, settings 0x20, watch 0x21/0x22), init pins them, and inputd reads the same constants — its three `new_with_slots(5, 6)` copies, the `new_with_slots(3, 4)` literal and the three `0x2x` consts are gone, as are init's now-dead `INPUTD_WATCH_*`. Ratchet 181→176 declarations / 60→59 files. Proof: `just test-all` green 2026-09-10 (8 lanes) — zero pin failures, and the input chain runs over the declared slots (`inputd: chain I5 delivered to windowd` → `windowd: chain I6 input recv`). **P4c gpud + the fleet-wide MMIO slot delivered 2026-09-10**: gpud declared (pure server, `slots::gpud::SERVER` 3/4) and pinned by init; its `GPUD_RECV/SEND_SLOT` read the declaration, `GPU_IRQ_NOTIFY_SLOT` now names `CTRL_SLOTS.recv` (the reuse it always was), and `DEVICE_MMIO_SLOT` became ONE fleet-wide constant — the number 48 lived in seven places (init + virtioblkd + rngd + timed + gpud + two selftest probes) and now lives once. ⭐ With gpud migrated, init's order-based `try_transfer` helper lost its last caller and was DELETED: every capability init hands a service now lands in a declared slot. Ratchet 176→166 declarations / 59→56 files. Proof: `just test-all` green 2026-09-10 (8 lanes) — zero pin failures, display path intact (`gpud: features=os-lite,virgl` → `fb handoff to gpud ok` → pixel proof). **P4d hidrawd delivered 2026-09-10**: hidrawd declared (pure producer — pushes normalized HID events to inputd, exposes no endpoint), its route pinned, its `CTRL_*` copies and the literal `[50, 51, 52]` input-window array replaced by `INPUT_MMIO_SLOTS` (init held the same block as a base constant). touchd needed no migration: it holds no capability beyond the control channel — recorded rather than invented. Ratchet 166→163 declarations / 56→54 files. Proof: `just test-all` green 2026-09-10 (8 lanes) — zero pin failures, HID chain intact (`hidrawd: virtio-input … ready` → `inputd: chain I5 delivered to windowd`). **P4e (app-child half) delivered 2026-09-10**: the SPAWNED-APP-CHILD capability table is declared once (`slots::app_child`: windowd 5/6, payload VMO 7, events RECV 8, reply inbox 9/10, svc base 11, events SEND 14, atlas VMO 19, minidump statefs 7/8). It lived THREE times — execd's grant constants, the app-host's "fixed constants" and `nexus-sdk-routes` — with each side's comment pointing at the other side. execd, app-host (incl. the three bare `WINDOWD_SEND_SLOT = 5` literals in the effect modules) and sdk-routes now read the declaration. Ratchet 163→146 declarations / 54→49 files. Proof: `just test-all` green 2026-09-10 (8 lanes) — app launch and mount unaffected (`APPHOST: mounted hash=`, pixel proof). **P4e-2 delivered 2026-09-10**: execd's OWN table is declared — it had no `ServiceSpec` at all, so every slot it owns came out of TRANSFER ORDER: server 3/4, reply inbox 5/6, logd 7, the delegated windowd route 8/9, bundlemgrd 10, the recv-wake probe endpoints 11-14 and the twelve named routes it resolves for spawned app-hosts (15-24). The order WAS the contract, written in comments ("keep this block FIRST: the probe slots are POSITIONAL", "ARM END on purpose: transfers here must never shift earlier positional slots") and backed by `probe_dump_cap_slots()` in execd — a diagnostic whose only job was to name a drift after it had already produced a dead handshake. init pins all of them; execd, `child_grants` and the probe binary read the same constants. ⭐ The pre-grant pass (`distribute_server_pair_for`) — which is what actually hands execd its server pair — is now declaration-driven for every migrated service and never falls back to an order-based transfer on a failed pin (a witness instead of a service listening on an unknown slot). `Execd → Logd|Timed|imed-osk|Windowd` were provisioned but never declared, so the host cross-checks could not see them; they are in `REQUIRED_ROUTES` now. Structure: slots → `slots.rs`, execd's arm + named routes → `bootstrap/execd_wiring.rs` (`wiring.rs` 1784 → 1586 LOC). Ratchet 146→134 declarations / 49→46 files. Proof: `just check` + `just test-host` green; all 8 lanes green 2026-09-10/11 (smp1, visible pixel 40.09 % / diff 22.18, reset, ota-flip, ota-bundle, -resume, -delta, -backstops) with zero `init: FAIL declared slot`, `execd: recv-wake probe ok` + `SELFTEST: exec child blocking recv wake ok` (probe endpoints 11-14 and child slots 5/6 through the declaration), `APPHOST: mounted hash=` (app launch via the delegated windowd route + bundlemgrd payload), `execd: app route granted svc=session|time`. HONEST NOTE: the lanes did not run as one `test-all` — the host's memory watchdog SIGTERMed the aggregate run four times (desktop memory pressure, not the lanes: a lane measured 726-845 MB), so they were completed individually; that failure mode is what TASK-0325 turned into a witnessed verdict. **Then proven in ONE run:** cold `make clean` + `just test-all` green 2026-09-11 (rc=0, all 8 lanes, visible pixel 40.09 % / diff 22.18, zero `FAIL declared slot`, `execd: recv-wake probe ok`, `APPHOST: mounted hash=`). **P4f COMPLETE 2026-09-12** — P4f-1a/1b (the generic arm's fifteen services + the block plane), P4f-2 (policyd, keystored), P4f-3 (updated, bundlemgrd), P4f-4 (netstackd, dsoftbusd, metricsd), P4f-5 (the proof harness) and P4f-6 (closure): the ratchet reached 0 and became an absolute gate; init has neither an order-based capability transfer nor a clone taken for one left. |
| P5 | Stage fence | `stage_fence.rs`; `ServiceSpec.stage`; entry hook waits; delete `yield_()` sync + `resume_drivers` order; `check-init-sync.sh` | In Progress — **P5-a (kernel floor) delivered 2026-09-12**. The fence had a security hole the stage design depends on: `fence_id_from_cap` checked only the capability KIND, so ANY holder could signal — a child could have released `ShellVisible` for the whole fleet. `Rights::WAIT` now exists beside `MANAGE` (kernel + `nexus-abi` mirror), `fence_create` mints `MANAGE \| WAIT`, `fence_signal` requires `MANAGE`, `fence_wait` accepts `WAIT \| MANAGE` (so `nexus-workpool`, which transfers its fences with `MANAGE` and both waits and signals, is untouched). Proof in the ladder: `KSELFTEST: fence transfer ok` — create, derive a WAIT-only copy, wait succeeds, signal refused (`SysError::Capability`). ⭐ The second half closes the P0 evidence item: the progress epoch behind `is_stalled` is bumped by EVERY trap (a timer tick included), so it could never tell a wedged fleet from a quiet one. `liveness::quiet_stall_witness` names the wedge — no user dispatch for 2 s AND every online hart idle AND ≥1 task blocked — latched once per boot and FAIL-shaped (`KSELFTEST: liveness snapshot FAIL quiet-stall since_ms= harts= blocked=`) so the harness FAIL gate reds the lane instead of hiding it behind a missing marker further down. Measured silent in a healthy smp1 boot. Structure: the ratchet forced real splits rather than a baseline bump — `selftest/fence.rs` (both fence proofs) and the witness moved next to `dump_snapshot` in `diag/liveness.rs`; `selftest/mod.rs` 2503→2344 (baseline 2422), `cpu_main.rs` 615→562 (limit 600). RFC-0093 §3 needed NO amendment: it already scoped this change and named both markers. RECORDED, not fixed here: the capability property tests (`cap/tests_prop.rs`) — including the derive-subset invariant that is exactly what makes a WAIT-only copy safe — execute NOWHERE. `mod cap` is `#[cfg(target_os = "none")]`, so a host `cargo test` never compiles them and the cross build is not a test build; the kernel names this as an open host-testability item (`lib.rs:320-322`). Widening their rights generator (`0u8..16`, which cannot reach the new bit 4) would have added ZERO real coverage, so it was left alone: the subset rule is proven for real by the ladder instead, where `KSELFTEST: fence transfer ok` derives the WAIT-only copy through the actual `CapTable::derive`. **P5-b (the ladder) delivered 2026-09-12.** `Stage` (1-4) + a MANDATORY `ServiceSpec.stage` on all 29 services in the topology; `STAGE_FENCE_SLOT` (0x38) fleet-wide. init creates ONE fence per boot and pins it `Rights::WAIT`-only into every child AT SPAWN — so the cap is there before the task is ever resumed and the early-running invariant holds by construction. ⭐ Two gaps the compiler found, both real: the volume-spawn pass and the RESPAWN path provision children by hand, so a restarted service would have had an empty fence slot — the barrier would have held only until the first restart. New verb `@stage <label>` on the same one-way frame as `@ready` (`MAX_SERVICE_NAME_LEN` 48 carries it unchanged — no new opcode); only windowd may report, identity is the control channel, others get `!stage-deny`, bad labels `!stage-unknown`. The responder is the ONLY signal site and prints `stage: <label>` there; the two fixed-position prints in the orchestrator are DELETED. ⭐ FUND: the first barrier rule (`CORE ∩ includes`) never opened — CORE contains the proof harness, which never calls `ready()`. Measured against the boot: the only services that never print `init: up` are exactly the specs with `exposes_server: false` (selftest-client, hidrawd, touchd). The rule is now declared truth, not a list: a member must run under the target, must EXPOSE A SERVER (you can only wait on something that serves), and must belong to the tier — guarded by `test_reject_barrier_member_without_a_server` + `platform_barrier_is_not_vacuous`. Host tests 53 green (8 stage tests incl. the deadlock guard and `recovery_reaches_every_stage_without_a_display`); smp1 boot-proven in order: `stage: platform` fires right after the LAST core `init: up` and before non-core ones, `display-ready`/`session-start`/`shell-visible` after windowd's reports. All four registered in the proof manifest (none ever was) and gated in both ladders. **P5-c (the barrier binds) delivered 2026-09-12.** `nexus_service_entry::os::bootstrap` waits for the service's declared stage prerequisite BEFORE `entry()`. That is the one funnel every OS service enters through (`declare_entry!` sets the name, then calls it), so the identity needed to resolve `spec_for` is already published — a process the topology does not declare (a spawned app child) holds no fence and waits for nothing. ⭐ The fence decides the ORDER; a 2 s liveness deadline decides only how long a broken boot may stay SILENT: on it the service prints `FAIL stage wait svc=… stage=…` once and keeps waiting, so the barrier still holds but a stage that never opens cannot hang the fleet anonymously. ⭐ The 22 `yield_()` in init were CLASSIFIED, not swept: 4 were barriers and are deleted (incl. one in `wiring.rs` that could never order anything — the services it meant to release were suspended), 18 are backoff inside deadline- or attempt-bounded IPC loops and stay; a blanket deletion would have broken every retry path and the fault fixture's deliberate park. ⭐ The hand-sorted driver resume list is GONE, and the boot proves the order was never the real contract: the drivers now resume in channel order — **hidrawd FIRST**, the exact case the old comment said would leave the screen black — and the display chain still comes up (`windowd: ready` → `present ok (seq=1 dmg=1)` → `systemui: first frame visible` → `launcher: first frame ok`). Dependencies are carried by parked routes (P3) and the fence. `check-init-sync.sh` gains three rules (no yield in init's orchestration files, no yield after a resume, no hand-ordered driver list), each SELF-TESTED against an injected violation (EXIT=1) and clean on the real tree (EXIT=0) — no gate that passes vacuously. Proof: `just check` green incl. the new `[PASS] stage-fence`; smp1 boot-proven — ladder at platform/display-ready/session-start/shell-visible, ZERO `FAIL stage wait` (the abilitymgr/sessiond deadlock I flagged as this package's biggest risk did not materialise), FAIL gate only the allow-listed dsoftbus pair. NEXT P6 (handoff contract v2: explicit reveal, seq acks, delete the present-ack lease heuristic). |
| P6 | Handoff contract v2 (windowd↔gpud) | explicit reveal (delete time caps/3-pixel probe), seq acks (delete lease/stall recovery), kernel display mode everywhere (delete windowd/inputd mode polls), readback off the scanout RT, `display: first scanout ok` on `STATUS_REVEALED` | In Progress — **P6-a (one display-mode source) delivered 2026-09-13.** The mode is read from `boot_display_mode()` (fw_cfg SSOT) and clamped by ONE policy now living in `nexus-display-proto`; it used to live in gpud alone, which is exactly why windowd and inputd had each grown a QUERY PROTOCOL to ask someone else. Both are retired: `OP_GET_DISPLAY_MODE` (windowd→gpud, 1280×800 fallback on EVERY failure path — a wrong mode could latch for a whole session) and `OP_GET_VISIBLE_MODE` (inputd→windowd, up to 100 polls at 200 ms while a 1280×800 space stood, so early clicks could land in a different space than windowd hit-tests in). ⭐ The layout maximum existed FOUR times (windowd, gpud, inputd, systemui's `PRESET_LAYOUT_MAX`) — one home now. gpud names a disagreeing device, windowd names an unconfigured boot. Dead `DisplayServerRuntime::new()` deleted. New gate `check-display-ssot.sh` with a `#[cfg(test)]`-aware scanner + fixture self-test, which caught a real defect in my own rule before it shipped (`\bDISPLAY_MODE_RETRY\b` can never match `DISPLAY_MODE_RETRY_NS`). LOC ratchet met by deletion, not by bumping (−346/+196 lines). NEXT P6-b (wire v2: attach ack 21 B, present `seq`, ack `[status, seq]`, and the lease/STALL heuristic dies with it), P6-c (reveal handshake), P6-d (readback off-scanout). **P6-b + P6-c (wire v2 + reveal handshake) delivered 2026-09-14 as ONE package** — they touch the same seams. Present `[op, seq]` / ack `[status, seq]`, attach ack 21 B with the mode as a CROSS-CHECK against P6-a's one source; `OP_REVEAL`/`STATUS_REVEALED`. windowd credits a present only against an outstanding seq (`present_acks::PresentWindow`, a crate-root host-tested module like `presentation_state` — the compositor is OS-only, and a negative test that never compiles proves nothing; four `test_reject_*`). ⭐ Deleted in the same package, because each existed only to guess at a fact the wire could not carry: the present-ack LEASE (`present_lease_expired` zeroed the in-flight count when acks went quiet — credit without evidence), the STALL watchdog + recovery, `FIRST_HANDOFF_DEADLINE_NS`, the 3-pixel `plane0_has_content` probe, both reveal time caps (0.5 s / 1.2 s), the three reveal-marker variants, and the 'reveal kick' (a re-present on cursor upload — the cursor was the old gate's signal). ⭐ The 'visible' markers moved to the `STATUS_REVEALED` ack: they used to print BEFORE the cursor was even uploaded; now `display: first scanout ok` → `systemui: first frame visible` → `ShellVisible` → `windowd: present visible ok` follow `windowd: desktop revealed (seq=2)`, a pixel-backed event. smp1 boot-proven in that order, zero new witnesses, every deleted heuristic absent from the log. Structure: gpud `reply.rs` (reply encoding + `present_status`), `service.rs` 1057→1012, `gl_scanout.rs` 1264→1214. TRAPS: (a) an item-deleter that checks its OWN line instead of the previous one leaves docs/`#[test]` orphaned and eats the next line's indentation — audit every deletion site; (b) `rustfmt` re-wrapping long paths grew a file back over its baseline — import the constant instead. NEXT P6-d (readback off-scanout, `gl_probe.rs`). ⭐⭐ THE PIXEL PROOF, NOT A MARKER, CAUGHT THE TRUTH — and corrected me twice. The first `test-all` failed only `visible`: `desktop is black (14.77 %)`. The PNG was the greeter dimming the wallpaper (honest, TASK-0065B). A second boot of the SAME code measured 40 % — the bare wallpaper, no greeter — at the same marker. Measured cause: a RACE between the greeter mount and the reveal present (`sessiond: greeter` precedes the reveal in both logs, 787<839 vs 787<791; only the distance differs). Both frames are legitimate non-black desktops; the judge's 30 % floor was calibrated on the bare-wallpaper case and rejected the greeter case. Judge re-anchored (≥5 % AND mean luma ≥2; black ≈0/0; every metric printed), verified against the real greeter JSON (pass) and a synthetic black one (fail). What STANDS from the old flow: the heuristic revealed on a gpud self-tick BEFORE windowd's first present (762<765), a wallpaper-only reveal by construction. What does NOT stand: my claim that the handshake makes the greeter frame the first one — it does not; TASK-0065B ('greeter in the first revealed frame') conflicts with RFC-0093 §3 (`DisplayReady` < `SessionStart`). RECORDED for P8/P9 as a contract decision, not patched here. **P6-d (readback off-scanout) delivered 2026-09-14 — P6 COMPLETE.** `gl_probe.rs` is the ONE readback authority: host copy-region front RT → probe RT (`0xE9`, 256×64), transfer of the PROBE only; `scanout_sample` (a transfer of the live scanout) deleted, markers renamed `gpud: probe sample ok` / `FAIL probe black` / `probe sample unavailable` (manifest, postflight tool, RFC-0093 §5, ADR-0032 in one step). ⭐ The scanout RTs lost their guest backing altogether (2 × 4 MB of VMO arena with no reader once the transfer is gone) — `diag-os-strict` found the dead accessor, the end state was to delete the backing, not to keep an unread allocation; measured green in the `visible` lane before claiming. `check-display-ssot.sh` rules 5+6 (time-free `should_reveal`, no `virgl_transfer_from_host(` outside `gl_probe.rs`), both fixture-tested. ⭐ smp1 is NOT the proof for this seam — its gpud has no virgl, so the readback never ran there (0 hits in every smp1 log, before and after); proof is the `visible` lane: `gl flip on` → `probe sample ok` → `SELFTEST: display nonblack ok` → handshake seq=2, pixel 40.09 %. NEXT P7 (consumer polls). |
| P7 | Consumer polls deleted | windowd session probe/greeter watch/cursor wait; app-host content-rect re-drive | Done — **P7-a (the wait primitive + the request/response polls) delivered 2026-09-14.** `nexus_ipc::budget` is the ONE wait: kernel-blocking with the remaining budget of an absolute deadline (`send_until`/`recv_until`/`recv_matching_until`/`raw::*_budgeted`); `retry_ipc_until`, `Clock::yield_now`, `recv_match_bounded`, `Connection::call(max_iters)` DELETED; `RouteRetryOutcome::Rejected { status }`. Consumers converted (selftest routing/keystored/statefs/plumbing/soak/security/samgrd/bringup, updated ×4, bundlemgrd ×2 + fleet route helper, logd ×2, inputd bind, dsoftbusd slot probe, windowd `session_client` mechanics, app-host content rect ONE ask + waited sends/ack/boot push + payload slot). ⭐ FOUND BY THE WAIT ITSELF: four private `Client` adapters ignored `wait` and always received NONBLOCK (execd ctl inbox, keystored + selftest reply inboxes, samgrd v2 inbox) — the first waited boot failed keystored keygen and the greeter launch through them; all four replaced by `KernelClient::new_with_slots` over declared slots. ⭐ dsoftbusd was WEDGED in its 10 000-yield slot probe (Idle QoS) for the whole smp1 lane — it printed nothing after wiring in every previous run; unwedged, its single-VM session path reports `dsoftbusd: dual-node connect FAIL` (network family HOLD) → manifest + FAIL allow list with the tracking reference; unwedged it is also timing-flaky (an unsupervised `?` error exit after `rpc listen ok` failed one test-all's death accounting) → the daemon HOLDS by declaration (`hold_forever`: kernel fence wait, zero CPU, no death) at its 18 spin-forever failure sites and on a rejected bring-up. ⭐ The selftest's bare `IMG_APPHOST=4` spawn probe is DELETED (dead since TASK-0080D: no payload = fail-closed by design; it only ever bred an 8 s polling zombie); failing fast exposed a KERNEL FINDING: the app-host's `sys_exit` (endpoint close + image release) held the BKL 14 ms (`KSELFTEST: bkl budget FAIL max_hold=14ms nr=11`) — the same cost every app close pays; recorded for a kernel task (approval zone), not patched. Gate `check-wait-not-poll.sh` + `config/wait-not-poll-baseline.txt`, ratchet 60 → 39 (only shrinks; fixture-tested; retired symbols rule). ⭐ dsoftbusd's single-VM session (dev-only, 62 undeclared diagnostic literals, one of which — `udp bind rpc timeout` — failed a test-all's evidence assembly) HOLDS in proof boots by the kernel's fw_cfg boot mode; the selftest transport probe is time-bounded (2 s per wait) instead of 100 000 re-ask rounds (a live session loop held the ladder 47 s). Proof: `just check` green; `just test-all` EXIT=0 (10 lanes, 15 handshakes, 15 content rects on the first ask, pixel 14.77 %, `bkl budget ok`); measured wall time smp1 226→202 s, reset 211→189 s; nexus-ipc 42 host tests. **P7-b delivered 2026-09-14 (kernel + fleet, both approvals used).** KERNEL: death wakes — `exit_current_and_release` runs the last-peer scan (`syscall::api::eof_scan`, ONE scan for recv/cap_close/exit) for every endpoint the dying task held a SEND cap to and wakes EOF-opted receivers with `PeerClosed`; the endpoint OWNER is never its own peer; a SEND cap in flight inside a queued message counts as a live peer (⭐ found by the first waited boot: every statefsd policy check EOF'd against its own in-flight request). Host-tested predicates (`ipc_eof`, 4 tests), router accessors split to `ipc/eof.rs`, the parent wake folded into the funnel. PROOF `SELFTEST: exec child eof on exit ok`. USERSPACE: `nexus_ipc::exchange` = the ONE request/reply without a clock (`call`/`call_into`/`call_matching`/`send_request`/`recv_response`/`mint_reply_channel`); `route_with_nonce` = one waited ask, `Timeout` outcome deleted; wave 1 converted (selftest rngd/statefs/keystored/samgrd/security/soak/quic_os, `nexus_ipc::policyd`, updated ×2, bundlemgrd logd ack, windowd session client, app-host content rect/ack/boot push/sends, abilitymgr spawn reply, logd replies). ⭐ THREE DEADLOCKS the 250 ms budgets had hidden every boot, all "a runtime route ask while init waits on the asker": (1) twelve services asked init for their OWN server route at start-up (virtioblkd's ask blocked → block plane never served → init's volume query failed for every service) — declared pairs only now; (2) bundlemgrd asked logd/@reply/metricsd inside its first request handler — declared legs, metricsd declared as a route (`slots::bundlemgrd::METRICSD`); (3) `nexus_log`'s sink asked for logd on its first line — `configure_sink_logd_slots` is now the ONLY binding (ask-free), bound in bundlemgrd/execd/updated/dsoftbusd. ⭐ windowd's synchronous session probe at handoff is DELETED (impossible under RFC-0093 §3: sessiond starts after `DisplayReady`; it deadlocked the boot once asks had no clock); windowd's sessiond/settingsd/gpud/imed/abilitymgr legs = declared slots, the 20-round abilitymgr loop gone; the harness's dsoftbusd readiness gate (never-ready target) deleted. Gate rule 3: clock-bound wait forms ratchet (`config/timeout-baseline.txt`, 133 in 45 files at start; the converted files are at 0). smp1: EXIT=0, 214 `SELFTEST: … ok` (+1), 41 KSELFTEST, `bkl budget ok`; `just test-all` EXIT=0 (10 lanes, eof-on-exit in every lane, 16 handshakes, pixel 14.67 %). RECORDED: init's responder is starved while init itself blocks in a synchronous exchange (volume query, policy check) — with clock-free asks every such window is a deadlock waiting for a runtime ask; the end state is an init that never blocks synchronously (waitset, P7-c/P8). **P7-c delivered 2026-09-14.** windowd on ONE waitset (server + gpud replies + settings push + session push + abilitymgr replies); the pacer timer, the 500 ms idle tick, the self-paced fallback, the session probe cadence, the greeter login watch, the launch-reply poll and the pacer-slip histogram are DELETED. Present completions (gpud replies) clock every animation (`on_frame_completed`), the ring depth is the throttle (`flush_pending_damage_if_slot_free`), `keep_frame_clock_alive` keeps a pointer-rect present going while a pulse client / coast / spring / wait ring animates without queued damage. sessiond pushes state (`OP_WATCH`, nexus-wire goldens; declared `SESSION_WATCH_RECV/SEND` 0x42/0x43, `NamedSlot::SessionWatch*`, init pre-mints + pins like the settings watch); windowd subscribes once and applies pushes from the waitset — decision and login are events. smp1: EXIT=0, 214 ok, `session watch subscribed` → `greeter on (dsl)` by push, all four stages, reveal. **NEXT (user decision 2026-09-14: "no polls, no timeouts — declarative, factory-minted, reactive"):** **P7-b — zero timeouts in request/response.** (1) One reply endpoint PER EXCHANGE, minted from the endpoint factory (`ipc_endpoint_create_for`, the pattern execd/init use for event channels) and moved with the request (CAP_MOVE); the waiter blocks on ITS endpoint — the shared `@reply` inbox, `ReplyBuffer`, nonce filtering and "foreign frames" are deleted. (2) Death wakes, not the clock: `sys_exit` closes the dying task's endpoints and wakes blocked peers (`PeerClosed` → `Disconnected`, kernel behaviour today) — a client never needs a deadline to learn its peer is dead; a live-but-silent peer is a supervision truth (ADR-0057), never a client timer. (3) Gate: `Wait::Timeout`/`deadline` forbidden in request/response code — only pacing (timer cap) and the kernel liveness witness touch a clock. **P7-c — one waitset per service** (server + minted reply endpoints + timer cap + fences), pushes as events: sessiond pushes session state → windowd's 250 ms session probe, 500 ms greeter watch, 500 ms near-idle settings tick and the launch-reply drain die; cursor wait ends on the launch reply. **P7-d — fences for shared-memory handshakes** (execd mints the payload-header fence, WAIT-only copy to the child, bundlemgrd signals after the header write) and netstackd readiness as a notify endpoint in the waitset (no `WOULD_BLOCK` re-ask); ratchet → 0 and the ratchet file deleted. RECORDED for the app-host: the payload HEADER poll on the VMO is a shared-memory handshake with no IPC event (bundlemgrd writes header-last) — a fence would make it a wait. **P7-d delivered 2026-09-14 — P7 COMPLETE (kernel + libs + scripts/config approvals used).** ⭐⭐ THE PIXEL PROOF CAUGHT P7-c: P7-c's faster windowd lost the greeter-vs-reveal race EVERY boot (`diff vs splash 0.02` — the bare wallpaper; before, the slow loop had won it by luck at 8.71). The recorded contract decision is made here, not deferred: the REVEAL is sent when the desktop is complete AND the session's desktop surface (greeter / shell) presented its first frame (`present_acks::RevealGate`, `test_reject_reveal_before_session_content`, `test_reject_reveal_before_display_complete`) — TASK-0065B's "login in the first revealed frame" holds by construction, RFC-0093 §3's stage order is untouched (the reveal is a later, content-backed event); pixel proof `diff 31.76` every boot. ⭐ Timer-notify pairs (`NamedSlot::TimerNotify*`, declared + pre-minted like the watch channels; app children's pair minted by execd) replace every recv timeout: inputd's 16 ms tick, settingsd's 50 ms persist tick (+ `REPLY_TIMEOUT_NS` deleted: a PUT in flight ends with statefsd's answer or its death), gpud's `SPIN_DEMO_PERIOD_NS` recv deadline (`FrameClock` = a synthetic vblank, disarmed once revealed), the app-host's 12 ms animation self-pace (frame pulses are requested with WAITED sends and carry every animation) and its minute clock. ⭐ KERNEL: peer death reaches waitset waiters — the last-peer scan latches `eof_pending`, `waitset_wait` reads a latched member as READY, a send or receive clears it (router test); without it a closed window never reached an app parked on a waitset. ⭐ bundlemgrd `OP_ARM_VMO` + `[status,len]` replies AFTER the header write (nexus-wire goldens; `armed_vmo` per KERNEL sender, bounded, `test_reject_take_by_another_sender`/`test_reject_arm_beyond_capacity`): init's spawner, execd (waits BEFORE resuming the child) and packagefsd wait for the answer; the app-host reads its header once — 3 × 200 000-yield header polls + the 8 s payload budget gone. ⭐ FOUND BY THE FIRST WAITED ATTACH: init resumed the core BEFORE distributing their legs (`wire_services` ran after `resume_core`; the priority-wire of windowd/inputd must precede it) — bootctld's 8 × 1 s attach retries had hidden a leg pinned after the task ran; the pass now runs before any resume, so §4's "pinned before the task runs" is finally true for every service (wiring is 9 ms). logd renders subject verdicts on init's `stage / shell-visible` append (an event) instead of a 1 s quiet period; bootctld attaches ONCE; metricsd/statefsd replies wait for queue space; abilitymgr/samgrd/dsoftbusd/imed/ingressd/updated/execd-probe/recv-wake child/dsoftbusd remote proxy and 14 harness probes are waited exchanges on declared legs (runtime route asks in abilitymgr/updated/samgrd/packagefsd gone). Gate at ZERO: polls 35 → 0, clock-bound wait forms 132 → 0, both baseline files deleted, the transport files excluded by path. Structure: inputd `os_lite/wait.rs`, gpud `frame_clock.rs`, app-host `probe/timer.rs`, bundlemgrd `armed_vmo.rs` (LOC ratchet met by splitting, never by bumping). Dependency: `rustls` 0.23.38 → 0.23.45 (RUSTSEC-2026-0285 blocked `just check`). ⭐ FOUND BY THE FIRST `test-all`: the harness's 51 runtime route asks each made init run a bounded policy exchange — under the `ota-bundle` load one answered `!route-deny … (policy unavailable)` and a probe lost its marker; the harness now takes its DECLARED slots (`route_with_retry`), the routing phase alone still asks and cross-checks (`route_checked`) — init's bounded waits are P8. RECORDED, not changed: `svc_call`'s DSL `timeoutMs:` budget is APP semantics (an app-visible error after N ms), not OS plumbing; execd's recv-wake probe keeps its deadline-bound receives as the proof instrument; init's own bounded loops (`grant_rtc_mmio_to_timed`, the handshake) are P8 (init on a waitset). PROOF: `just check` green (gate `[PASS] wait-not-poll: … zero poll-against-clock functions, zero clock-bound wait forms`); smp1 EXIT=0 — 214 `SELFTEST: … ok`, 41 KSELFTEST, `bkl budget ok`, boot `total_ms=1256` (1262 before; the wiring pass folded into the pre-grant phase, `wiring_ms=0`); visible EXIT=0 — pixel proof `diff vs splash 32.44` (greeter in the revealed frame); `just test-all` EXIT=0 (10 lanes: smp1, reset, visible, ota ×7; eof-on-exit and the reveal handshake in every boot, 18 each). NEXT P8/P9. |
| P8 | Closure docs/baselines/gates | LOC baseline, docs (RFC-0069 §4 implemented, RFC-0013, ADR-0041/0050), CI parity; init without clocks | Done — **delivered 2026-09-15 (kernel + scripts/config + docs/rfcs approvals used).** INIT WITHOUT CLOCKS: every exchange init makes is waited (the answer or the peer's death — RFC-0079 EOF on init's own reply endpoint, which is also the policy reply channel): the three policy checks, the MMIO grants (one policy exchange each; the 1 s `yield_()` retry loops are gone — an unanswered authority is `mmio policy unavailable`, fail-closed), the RTC grant, the bundlemgrd slot switch, updated's health/status, the boot-attempt handshake (20 × 500 ms → ONE exchange) and the supervision-persist statefs exchange. THE RESPONDER waits on ONE waitset with no safety net: every control channel + init's own timer-notify endpoint (`responder_clock.rs`: a self-minted endpoint, a kernel one-shot armed at the earliest scheduled respawn via `SupervisedChild::due_ns`/`Respawner::next_due_ns`/the fault probe's, disarmed otherwise). A child's death reaches the waitset through the kernel's EOF latch on that child's control endpoint — ⭐ KERNEL: the latch is set whenever no foreign sender remains, whether or not a sender was ever seen (the receive still decides EOF by the live rule; the latch only ends the wait), otherwise a child that died before its first write could not wake its supervisor. Boot-proven: `SELFTEST: supervision restart ok` (fault probe restarted 4× on the timer, `crash-loop cap ok`), `init: service restarted name=pinched` ×2, `init supervision sweep ok`, boot `total_ms=1257`. Gate `check-wait-not-poll.sh` scans `source/init` too (zero fleet-wide). CLOSURE: `config/loc-baseline.txt` ratcheted to the real sizes (24 entries; `responder_clock.rs` split out); RFC-0069 → Implemented (manifest = `nexus-service-topology`, §4 stages = the RFC-0093 §3 fence), RFC-0013 (readiness = `@ready`), ADR-0041 (decision stands; mechanism = reveal handshake + `FrameClock`), ADR-0050 → Accepted/implemented; CI parity RECORDED, not changed (CI: `headless` + `ci-os-smp`; `test-all`: smp1/reset/visible/OTA ×7 — the pixel proof needs a GL host). PROOF: `just test-all` EXIT=0 (10 lanes: smp1 visible reset ota-flip ota-bundle ota-bundle-resume ota-bundle-delta ota-tamper ota-downgrade ota-fallback); smp1 214 ok / 41 KSELFTEST / boot 1257 ms; visible pixel diff 32.72; gate zero fleet-wide incl. init. |
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

**P4f-6 delivered 2026-09-11 (closure):** the slot SSOT is closed. **Gate:** `check-slot-ssot.sh`
is no longer a ratchet but an absolute rule (baseline file deleted, 191 → 0): outside
`nexus-service-topology` (slots init provisions) and `nexus-abi` (slots the kernel installs) no
Rust source in `source/{services,drivers,apps,init,libs}` or `userspace/` may carry a capability
slot as a number — `const …SLOT/…_EP` literals (u32/Handle/Cap), `let …slot… = <literal>`,
non-zero literal `new_with_slots` arguments, literal `cap_transfer_to_slot` pins; every run first
proves the scanner on fixtures (6 shapes reported, declared forms ignored). The blind spots P4f-1b/
P4f-2 recorded (literal pins, literal call args, `let` literals, `Handle`/`Cap`-typed consts,
`userspace/` and `source/libs` outside the scan) are all covered. **What the widened scan found:**
(1) ⭐ **statefsd's audit trail never reached logd** — `append_logd_audit` (ten callers: access,
quota, ABI, envelope, compaction, budget, degrade) sent to a literal slot 8 that init never
provisioned (statefsd's spec had declared only its policyd leg, before and after P4f-1a); the
send errors were ignored. The leg is declared (`slots::statefsd::LOGD`, late-grant band: statefsd
runs from wave 1 on) and provisioned; the selftest reports delivery after the ABI refusal
(`SELFTEST: statefsd audit delivered ok|dropped`, best-effort like the learn record, not
ladder-gated). (2) Kernel-installed slots were copied as literals in init (factory 1), execd's
host backend and two selftest probes (bootstrap 0) — `nexus_abi::{BOOTSTRAP_CAP_SLOT,
INIT_ENDPOINT_FACTORY_SLOT}`, documented as mirrors of neuron's `spawn_inner`/`FACTORY_CHILD_SLOT`.
(3) The control channel was a PARAMETER of every route ask: `route_with_nonce_budgeted(name,
1, 2, …)` — 26 call sites passed slots 1/2 (literally in execd, packagefsd and `nexus-ipc`'s own
policyd helpers; `nexus-ipc` and `nexus-service-entry` held further copies). The helper reads
`CTRL_SLOTS` now; every copy is gone. (4) hidrawd's IRQ-notify/idle-park endpoint was
`Cap = 2` twice (the control channel's RECV half), nexus-net-os's MMIO slot a `let … = 48`,
dsoftbusd's observability inbox `let 0x6/0x5`, metricsd's retention client a recv literal `0`.
**Cap hygiene:** ⭐ init took **19** capability clones before transferring them (execd's legs 10,
windowd/inputd priority wiring 4, the OSK pair 2, the policyd reply cap 1, the samgrd registry
pair 2) on the belief — written into several comments — that a transfer MOVES the capability. It
duplicates (P4f-1a finding), so every clone was a leaked slot in a table that runs near its ceiling
by wiring time. The originals are pinned (the OSK endpoint is closed after wiring with the other
wired endpoints); `test_reject_clone_leak` scans `src/bootstrap/` and rejects a `cap_clone` whose
result is neither moved in a message nor closed (its scanner self-test covers the three leak shapes
removed here and the four legitimate CAP_MOVE shapes). **Deleted:** the "no minted pair → fresh
endpoint" fallbacks in `distribute_server_pair_for` and the generic arm (a declared server without a
minted pair is now `init: FAIL server pair not minted svc=…`), init's unused `slot_map.rs` (a
second slot table with wrong numbers and no user), execd's `probe_dump_cap_slots` (an order-drift
diagnostic for a class that no longer exists), `INPUTD_SETTINGS_SEND_SLOT` (a copy of
`slots::inputd::SETTINGS_SEND`), and the unscheduled VMO-share probe with its hand-built consumer
ELF, which agreed on a fixed child slot 23 by convention. ⭐ Finding: TASK-0031 is Done on an OS
proof (`SELFTEST: vmo share ok`) that has run in no lane since the RFC-0068 exec migration —
corrected in its ledger, status left for review. `touchd` got a (capability-free) spec and
`test_reject_service_missing_from_specs` requires one for every service (`imed-osk` names a route
target, not a process). Not in P4f-6 (recorded): windowd's gpud route ask in front of the declared
pair is a readiness barrier — P5's stage fence deletes it; the selftest's route-ask retry and the
hand-copied `route_blocking` helpers stay P7.
**Proof:** `just check` green (the gate proves its own scanner on fixtures first); smp1 smoke green;
`just test-all` green in ONE run (exit 0, started detached): gates + smp1, visible (pixel 40.09 % /
diff 22.18), reset, ota-flip, ota-bundle, -resume, -delta, ota-backstops (tamper, downgrade,
fallback). In EVERY lane: 0× `FAIL declared slot`, 0× `server pair not minted`, 0× route divergence.
⭐ `init: statefsd route->logd ok` in every boot and `SELFTEST: statefsd audit delivered ok` in every
lane that runs the policy phase (smp1, visible, reset, tamper, downgrade — the OTA lanes skip it):
the first evidence that a statefsd audit record reaches logd. Unchanged after removing the 19
clones: execd's nine named routes, `init: windowd priority-wired` + `init: inputd priority-wired`,
`init: execd route->imed-osk ok`, `init: selftest-client route->imed-osk ok`,
`execd: recv-wake probe ok`, `SELFTEST: service restart ok`, `APPHOST: mounted hash=`, the eight
`SELFTEST: ipc routing … ok` markers. Lane peaks 742 MiB-1.69 GiB. The only reds are the known ones:
the two allow-listed dsoftbus markers and the intended `nxboot: verify FAIL` of the tamper and
downgrade lanes.

**P4f-5 delivered 2026-09-11 (architecture review first):** the proof harness (`selftest-client`)
has a `ServiceSpec` — 23 routes, its reply inbox and `NamedSlot::FwCfg` declared at the numbers of
the old order-based layout (behaviour-neutral by construction; every number the harness hardcoded
was measured from a boot log first). It runs from wave 1 on, i.e. BEFORE `wire_services`, so init
now pins its legs right after the server-pair distribution (`declared_routes::wire_proof_harness`)
— resumed implies wired for the harness (the P4f-1b invariant); the smoke log shows its 23
`init: selftest-client route->… ok` lines before the fw_cfg grant and before any other service is
wired. The generic arm's inbox + route block became ONE function (`declared_routes.rs`) serving both;
reply-inbox targets resolve through `Endpoints::request_ep` (the minted pair, plus two stated
exceptions: the per-consumer OSK clone and the priority-wired inputd). Deleted: the 279-line
selftest arm, `gateway_route.rs`, `provision_selftest_imed_osk`, `wire_blk_deny_probe` (a separate
post-wiring pass that existed ONLY so one more leg would not shift the numbers the harness
hardcoded), the literal fw_cfg slot, `net_selftest_rsp`; `wiring.rs` 835 → 426 LOC. **Least
privilege:** netstackd, ingressd and virtioblkd answer on the harness's CAP_MOVE cap, so their RECV
halves (never read) are no longer granted; the blk deny probes still hold no block-plane grant.
**Harness:** reads the declaration everywhere (the eight-service table in `route_with_retry`, the
reply inbox in eight files, rngd, the logd sink `0x15`, fw_cfg, a keystored fallback that tried one
literal pair in BOTH orders, the vfsd fallback, CTRL `1, 2` in five route asks). ⭐ **Hollow routing
markers made real:** `SELFTEST: ipc routing <svc> ok` followed a hardcoded slot table and proved only
that a client object could be built; `route_with_retry` now asks init and checks the answer against
the declaration (`nexus_service_topology::route_matches`, host test
`test_reject_route_answer_diverging_from_declaration`), a divergence prints the new
`SELFTEST: route diverges from declaration FAIL svc=` and the FAIL gate stops the lane. Dynamic
grants stay in-band: `@mint-pair` and pinched's respawn re-grant (re-resolved by name, ADR-0057).
Ratchet 27/14 → 4/4. **For P4f-6:** the four remaining declarations are init's
`ENDPOINT_FACTORY_CAP_SLOT = 1`, `INPUTD_SETTINGS_SEND_SLOT = 0x20` (duplicates `slots::inputd`),
execd's host `BOOTSTRAP_SLOT = 0` and the seeded, unscheduled VMO-share probe's child slot 23
(`#[allow(dead_code)]` — declare or delete); the selftest's `new_with_slots(0, 0)` loopback probe
uses the kernel bootstrap slot; the "no minted pair → fresh endpoint" fallbacks. **For P7:** the
harness's route asks still retry a 500 ms `QueueEmpty` poll until init's responder runs (the stage
fence makes it one ask); `resolve_keystored_client` pings in a 128-round loop. **For P8:** init's
responder still prints selftest-specific route debug lines (`init: route samgrd rsp …`).
**Proof:** `just check` green; smp1 smoke green; `just test-all` green in ONE run (exit 0, started
detached): gates + smp1, visible (pixel 40.09 % / diff 26.03), reset, ota-flip, ota-bundle, -resume,
-delta, ota-backstops (tamper, downgrade, fallback) — 0× `FAIL declared slot` and 0×
`route diverges from declaration` in every lane, 23 harness route witnesses per boot, the six
`SELFTEST: ipc routing <svc> ok` markers (now checked against the declaration) in every full-ladder
lane, `SELFTEST: blk cross-partition deny ok` + `blk system volume deny ok`, pinched's three
restarts re-resolved (`init: route resumed svc=selftest-client -> pinched` ×3,
`SELFTEST: service restart ok`), lane peaks 743-1,118 MiB. HONEST NOTE:
`windowd: FAIL present-ack lease expired — presenting without credits` printed in ota-flip and
ota-fallback of this run. Measured against the stored September runs it is a pre-existing sporadic
event of the present-ack lease heuristic (ota-flip 9/60, ota-fallback 12/57, reset 13/61, smp1 11/101
runs) — the heuristic P6 deletes; the FAIL gate does not cover `windowd:` lines (recorded for P6/P8).

**P4f-4 delivered 2026-09-11:** netstackd, dsoftbusd and metricsd leave their bespoke arms for the
generic arm (243 lines of literal pins deleted; `wiring.rs` 1180 → 835 LOC). Declarations: netstackd
server 4/3 + inbox 6/5 + policyd 7; dsoftbusd server 4/3 + inbox 6/5 + netstackd 7, samgrd 9,
bundlemgrd 0xA, logd 0xF (CAP_MOVE) and packagefsd 0xB/0xC, statefsd 0xD/0xE (their shared response
endpoints); metricsd server 4/3 + inbox 6/5 + statefsd 7, logd 8. Nine edges init provisioned but
never declared are in `REQUIRED_ROUTES` now, so the host route/policy cross-checks see them. The
selftest arm's metricsd pins 0x21/0x22 are the SELFTEST's slots and move with P4f-5. **Findings:**
(1) ⭐ netstackd's facade server pair was granted TWICE — by transfer order at 3/4 in the pre-grant
pass and pinned at 5/6 by its arm (the facade listened on 5); it lives once, at the fleet's server
slots. (2) ⭐ **A route kind follows the target's reply discipline, not the endpoints init mints.**
netstackd answers every RPC on the caller's CAP_MOVE cap and never on a response endpoint (the facade's
`_svc_send_slot` was never used), so the "netstackd response endpoint owned by dsoftbusd" never carried
a byte; its only reader was dsoftbusd's direct-recv fallback, which could not have received anything.
dsoftbusd's leg is `ReplyInbox`; the endpoint and the fallback are deleted, and so is dsoftbusd's
pre-minted inbox (the generic arm mints it like every other). The selftest's twin `net_selftest_rsp`
goes with P4f-5. (3) dsoftbusd served its own pair after an UNBOUNDED `KernelServer::new_for("dsoftbusd")`
retry loop — it reads the declaration now. (4) The generic arm's reply-inbox bridge was a hand-kept
`ServiceId` → capability match that silently skipped any target nobody had added (it had no samgrd
target); it resolves the target through the minted-pair table (`Endpoints::server_pair`) — one lookup,
one uniform `init: <svc> route-><target> ok` witness per route, emitted as ONE atomic line because the
netstackd one is a ladder marker and byte-wise writes tear against the services running during wiring.
(5) With the last unmigrated spec declared, the order-based branch of `distribute_server_pair_for` is
DELETED — init has no order-based capability transfer left outside the selftest arm — and
`test_reject_partial_slot_declaration` lost its "not migrated yet" exemption. **Marker:**
`init: netstackd policy slots 7/8/9` → `init: netstackd route->policyd ok` (`scripts/qemu-test.sh` +
`docs/security/abi-filters.md`; slot numbers belong to the declaration, not to a ladder string).
**For P4f-6:** remaining positional declarations outside the selftest are `ENDPOINT_FACTORY_CAP_SLOT = 1`
(init's own table), `INPUTD_SETTINGS_SEND_SLOT = 0x20` (duplicates `slots::inputd`) and execd's host
`BOOTSTRAP_SLOT = 0`; the "no minted pair → fresh endpoint" fallbacks (`distribute_server_pair_for`,
`provision_server_endpoint`) have no reachable caller left — delete them and make a present server
without a minted pair a loud failure. **For P7:** `dsoftbusd: waiting for slots` (a 10 000-yield
`cap_clone` poll, a manifest marker) is vacuous once wiring precedes resume; dsoftbusd carries two copies
of the netstackd RPC layer (`os/entry.rs`, `os/netstack/rpc.rs`); its remote statefs proxy discards
non-matching frames it drains from statefsd's SHARED response queue (a reply-steal hazard shared with
the selftest). Ratchet 38/17 → 27/14. **Proof:** `just check` green; `just test-all` green in ONE run
(exit 0, started detached — the host memory watchdog killed only the waiter, twice): gates + smp1,
visible (pixel 40.09 % / diff 22.18), reset, ota-flip, ota-bundle, -resume, -delta, ota-backstops
(tamper, downgrade, fallback) — 0× `FAIL declared slot` in every lane, `init: netstackd route->policyd ok`
and `net-egress: enforced` in every lane (reset: 3 witnesses / 2 egress lines over three boots, the same
3/2 the pre-P4f-4 reset lane showed with the old witness), `SELFTEST: icmp ping ok`, egress deny/allow,
ingress allow, metrics retention + tracing spans ok, lane peaks 757-1,174 MiB. HONEST NOTE: dsoftbusd's
own runtime path is exercised by no green lane — none of its bring-up lines appears in any smp1 run
before or after this package, and the DSoftBus FAIL markers are allowlisted (network family on HOLD).
Its changes are proven by the OS-cfg build, host tests and init's wiring witnesses; its reply-inbox
slots kept their numbers and the removed code was unreachable.

**P4f-3 delivered 2026-09-11:** updated and bundlemgrd leave their bespoke arms for
the generic arm (updated's 114-line arm, bundlemgrd's 53-line arm, `wire_updated_vfs_leg` and
`updated_policyd_leg` deleted; the bridge gained the vfsd and bootctld targets). **Late-grant band
`LATE_GRANT_BASE = 0xE0`:** bundlemgrd runs from the core plane on and allocates volume-window VMOs
itself, so its reply inbox and logd leg — granted in the wiring phase — are declared from 0xE0 up,
above anything a service allocates itself (it resolves them by name; the old order-based transfer
put them wherever its allocations had left room). policyd's 0x9-0xB predates the band for a stated
reason (slots 1-8 all pinned before it resumes, no allocation before it is wired). With policyd,
bundlemgrd and virtioblkd all declared, the core plane pins directly and the transitional
`grant_server_pair` is deleted. **Two more unused grants removed (least privilege):** the
"bundlemgrd↔execd dedicated pair" — its request endpoint was minted for execd but never handed to
execd, bundlemgrd never sent on it; the selftest's "bundlemgrd may not route to execd" proof is
decided by policyd before the route-table lookup (responder order verified) and stays green — and
updated's dedicated bundlemgrd response endpoint at slot 6, which updated's own code called
"unused, we use reply inbox". updated's three client files each carried the same CTRL + reply-inbox
block citing a non-existent "slot_map SSOT"; all read the declaration. Ratchet 57/22 → 38/17.
**P7 addition:** bundlemgrd's `route_status` polls `ipc_recv_v1` against a deadline too (not caught
by the earlier `Client::recv(NonBlocking)` scan). Smoke: smp1 green, 0× `FAIL declared slot`,
`bundlemgrd: volume status served`, `packagefsd: mounted`, `SELFTEST: bundlemgrd route execd denied ok`,
`SELFTEST: bundlemgrd volume ok`, `init: up updated`, `SELFTEST: ota delta base deny ok`. **Proof:** `just test-all` green in ONE run
(exit 0), started detached from the agent task so the host memory watchdog could only kill the
waiter (it did, once): gates + smp1, visible (pixel 40.09 %), reset, ota-flip, ota-bundle, -resume,
-delta, ota-backstops — 0× `FAIL declared slot` in every lane; updated's OTA path intact
(`SELFTEST: ota flip ok`, `ota stage resume ok`, `ota bundle delta ok`, `bootctld: commit ok (slot=b)`,
`updated: bundle reused (name=…)`).

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
stage barriers can be called deterministic. **CLOSED 2026-09-12 (P5-a):**
`liveness::quiet_stall_witness` is that witness — no user dispatch for 2 s AND every online
hart idle AND at least one task blocked, latched once per boot, FAIL-shaped
(`KSELFTEST: liveness snapshot FAIL quiet-stall ...`) so the lane goes red instead of silent.
It stayed silent in a healthy smp1 boot; the next occurrence of the 1/16 stall will name
itself with a task snapshot instead of an empty UART.

Per package: host tests → `just check` → `just test-all` incl. the lanes named in the plan
(blast radius) → docs sweep → commit proposal (user commits).

## Not in this task

Compositor/present pipeline redesign; session UI/OSK/greeter design; network family
(HOLD); QEMU/virglrenderer changes. The 2D device path (`virtio-gpu-device`,
`display-gpu-pci` lane) stays as the ONLY path for non-GL devices.
