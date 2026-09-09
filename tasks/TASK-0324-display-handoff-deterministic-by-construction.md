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
| P1 | RFC-0093 + ADR-0062 + ledger/board | routing v2 (nonce, parked replies until `@ready`, fail-closed), `@ready`/`@stage`, `ServiceSpec.stage` + fence, declared slot map for services AND app children, handoff v2 (attach ack {seq, mode, content_rect}, `OP_REVEAL`/`STATUS_REVEALED`, seq acks), display mode from `boot_display_mode()`, pixel proof as display gate | Draft |
| P2 | `@ready` + `nexus_service_entry::ready()` + honest `init: up` | fleet-wide; `ready_table.rs`; delete `init: up` in orchestrator/volume_spawn; docs/manifest | Draft |
| P3 | Routing v2 | delete v1 `query_route`; nonce mandatory; `route_park.rs`; bounded-blocking replies; fail-closed policy; delete windowd alias guards | Draft |
| P4 | `nexus-service-topology` = one slot SSOT | crate move + slots; declared arm; per-consumer atomic sub-packages P4a windowd, P4b inputd, P4c gpud, P4d hidrawd/touchd, P4e execd + app children (`nexus-sdk-routes` view), P4f policyd/netstackd/bootctld/keystored/updated/dsoftbusd/bundlemgrd/metricsd/imed/selftest-client; delete `is_bespoke_wired` + all arms + slot-order comments; `check-slot-ssot.sh` | Draft |
| P5 | Stage fence | `stage_fence.rs`; `ServiceSpec.stage`; entry hook waits; delete `yield_()` sync + `resume_drivers` order; `check-init-sync.sh` | Draft |
| P6 | Handoff contract v2 (windowd↔gpud) | explicit reveal (delete time caps/3-pixel probe), seq acks (delete lease/stall recovery), kernel display mode everywhere (delete windowd/inputd mode polls), readback off the scanout RT, `display: first scanout ok` on `STATUS_REVEALED` | Draft |
| P7 | Consumer polls deleted | windowd session probe/greeter watch/cursor wait; app-host content-rect re-drive | Draft |
| P8 | Closure docs/baselines/gates | LOC baseline, docs (RFC-0069 §4 implemented, RFC-0013, ADR-0041/0050), CI parity | Draft |
| P9 | 8/8 visible boots + test-all + progress table | | Draft |

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
