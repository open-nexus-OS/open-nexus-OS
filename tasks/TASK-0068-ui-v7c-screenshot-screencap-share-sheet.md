---
title: TASK-0068 UI v7c: screencapd over the ONE pixel-readback authority (gpud readback + windowd geometry/secure-surface gate) + consent/caps
status: Done (2026-10-09 — the reference desktop's screenshot tool in our glass style: RFC-0095 Done, ADR-0071; `usb-visible` Print → drag → shutter, board rung `board-visual: screenshot`; recut with the operator 2026-10-08, see "Recut 2026-10-08" below)
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - ADR: docs/adr/0022-modern-image-formats-avif-webp.md
  - UI v4a compositor baseline (readback): tasks/TASK-0060-ui-v4a-tiled-compositor-clipstack-atlases-perf.md
  - UI v6a WM baseline (grab window): tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - Clipboard v2 (destination): tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - DSoftBus (peer share, optional): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Policy as Code (consent/limits): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Persistence (/state save): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Recut 2026-10-08 — the screenshot UI (binding; supersedes the 2026-09-09 rewrite below where they differ)

Operator decisions (2026-10-08): the screenshot UI follows the reference desktop's interactive
screenshot tool, built in our style. Print opens a fullscreen overlay over a FROZEN frame (the
screen is grabbed first; what you see is what you get, and the overlay is never in the image).
Outside the selection a translucent dark overlay (opacity, no blur). A floating glass panel at
the bottom centre, styled like our notifications with the round X at its top right: a top row
with three round mode buttons — Selection (drag to draw, drag to move, handles to resize),
Screen, Window (windows highlighted in place, a click picks one; the visible rect is captured)
— and a bottom row with the photo/film switch on the left (film drawn greyed out and disabled —
`.disabled(true)`, operator 2026-10-09 — until recording, TASK-0105), the shutter in the middle and the pointer toggle on the right. The capture is saved
as PNG to the user's Pictures/Screenshots and confirmed with a toast. Image copy to the
clipboard stays TASK-0087's (clipboard flavors). Not in scope: recording, editor, gallery,
share sheet.

## Delivered 2026-10-09 (what was built; supersedes the D1–D5 decisions below where they differ)

Contract: RFC-0095 (screen capture v1), ADR-0071 (readback in gpud, freeze in windowd, UI in
the shell); amendments RFC-0053 (the visible state's `capture` byte) and RFC-0072
(`OP_ARM_VMO` + `OP_WRITE_VMO`).

- **One readback authority (gpud `OP_READBACK = 15`; 14 is `OP_FRAMEBUFFER_REQUEST`).** GL:
  the front render target is copied into a capture resource backed by the caller's VMO and
  transferred back (`gl_probe.rs`, shared `copy_front_to_guest` with the display-truth
  probe); virtio 2D and the board's controller: a CPU copy of the display plane
  (`src/readback.rs`, host-tested). `READBACK_FREEZE` also makes the frame the base layer (GL:
  the wallpaper texture; CPU: the retained plane). A negative `OP_MOVE_CURSOR` position is "no
  pointer in this frame" for the GL build-up.
- **windowd's capture verb (`OP_SURFACE_CAPTURE = 32`)**: the pure machine
  `capture_gate.rs` (identity = screencapd's kernel sid; no freeze at the greeter; one capture
  at a time; `FREEZE_MAX_NS` 60 s, `READ_MAX_NS` 2 s) + `runtime/capture.rs` (the lent frame
  VMO, the parked reply, the pointer out of the frame for one present, the freeze from the
  moment the readback is queued, owed sends retried by the loop). The dormant
  `USE_DESKTOP_SHELL` switch is gone — `frozen` is the one runtime input of `window_scene`
  (windows AND overlays leave the composition). The sprite windowd shows is written behind the
  frame, so screencapd keeps no cursor copy.
- **screencapd** (`ServiceId::Screencapd = 34`; routes to windowd and vfsd; `svc.screencap`
  for the `shell`/`settings` bundle types via `nexus.permission.SCREENCAP` — the route is the
  capability, no policyd leg): `BEGIN` / `SHOOT` / `CANCEL` / `SHOT` / `PROBE`
  (`nexus_wire::screencapd`). Frame VMO + encoder scratch allocated once; the PNG goes into a
  worst-case-sized VMO (`png_encode::max_encoded_len`) and vfsd writes it in ONE transaction;
  `/Bilder/Screenshots/<stem> <YYYY-MM-DD HH-MM-SS>.png`, ` (n)` on a collision, the claimed
  name removed on a failed write.
- **png-encode** (`userspace/png-encode`): bounded-memory RGB PNG encoder; `encode_rows` pulls
  rows (a frame in a VMO is read row by row, never mapped).
- **Storage path, leak-free**: nxfs keeps ONE reused 64 KiB working buffer (every write used
  to leak 64 KiB on vfsd's never-freeing heap) and gains `write_from` (bytes pulled in one
  transaction); vfsd `OP_ARM_VMO` (the VMO, keyed by the kernel sender id) + `OP_WRITE_VMO`
  (pulls with `vmo_read`, closes the VMO before answering); vfsd's small allocations recycle
  (`small-object-free-list`).
- **Keys**: HID `0x46` (evdev 99 on the virtio lanes); inputd swallows Print / Shift+Print /
  Alt+Print into a one-shot `capture` fact (`stamp_key_facts`, tests `key_facts.rs`); windowd
  carries one-shot facts across staged samples (a pointer move in the same frame used to be
  able to erase a tiling chord) and pushes `OP_SURFACE_CAPTURE_KEY` to the desktop surface
  (retained until taken; no line per key); app-host fires `CaptureOpen` / `CaptureScreen` /
  `CaptureWindow` by name.
- **The DSL drag gesture**: windowd sends `INPUT_KIND_DRAG` to the surface that took the press
  while the button is held and `INPUT_KIND_RELEASE` at the end; app-host (`probe/drag.rs`)
  fires `DragStart` hit-tested at the press past a 3-px slop, then `DragMove` / `DragEnd` on
  THAT box (`View::fire_on_box`), with `device.dragX/dragY/dragStartX/dragStartY`. Reactive
  and lazy (operator review 2026-10-09: "compute only when needed"): windowd sends every
  applied sample (one per loop pass) and retries one a full queue refused on its next pass —
  the last position always arrives, no clock involved; app-host only NOTES a DRAG and computes
  the newest position once, right before the frame that shows it (start + first move, last
  move + end each dispatched together and laid out once); the repaint is the layout diff's
  rows, now tight for geometry-only changes (a layout container that moved damages nothing, a
  flat fill only the rows its coverage left or took — the dragged selection repaints the
  hole's rows, not the frame), and glass regions are re-declared only when a glass root
  moved. The release is discrete — windowd parks on it like on a tap (`Delivery::for_input`)
  — and carries the gesture's last position. One bounded `apphost: drag (…)->(…) hit=…` line
  per gesture (never in the keyboard overlay) is the live lane's anchor.
- **DSL fix**: `.width/.height/.min…/.max…($state.x)` — the checker accepted an Int
  expression and the emitter silently dropped it; it is now a LAYOUT dependency
  (`tests/dynamic_size.rs`).
- **The shell's screenshot UI** (`ui/components/capture/`, `composables/capture.store.nx`,
  the `SystemStore` — power, the system toast and the capture share one store because a
  reducer writes one): scrim bands around the hole (selection / picked window / screen), corner
  handles, windows outlined in place (back to front, so the front-most wins a click), the glass
  panel with the X, the three modes, photo/film (film greyed out and disabled — `.disabled(true)`, no press or hover — until TASK-0105), the centred shutter
  and the pointer toggle; the toast names the file. While it is up the live shell content
  leaves (the frozen frame shows it).
- **Modal edge** (found by the lane, 2026-10-09): a modal that closed and another that opened
  before one paint (the search's ESC, then Print) kept the depth, so app-host's sync — depth
  only — named no open; `modal_edge.rs` (pure, 4 host tests) compares the top modal's identity
  too. The injector also waits for the search's own close before pressing Print.
- **Privacy**: windowd's "input routed" lines are said once per boot (a line per tap logged
  the on-screen keyboard's keystroke timing); no capture line carries a name, a stem or
  pixels.

Proof: host — `capture_gate` (11), display-proto codecs, gpud readback copies, png-encode (57),
screencapd contract (7), nxfs `write_from`, vfs-types codecs, inputd `key_facts`, DSL
`drag_gesture` + `dynamic_size`, the shell's `shell_capture` (5). QEMU — every visible lane:
`screencapd: ready`, `SELFTEST: ui v7 screencap ok` (the selftest reads the shown frame
through the whole path); `usb-visible`: Print → drag → shutter → `screencapd: freeze ok`,
`windowd: capture freeze on`, `screencapd: saved (kind=area …)`, `windowd: capture freeze
off`, `SELFTEST: ui v7 screenshot ok`. `just test-all` green in one run on the final code
(2026-10-09; earlier runs that day were cut by the host's btrfs stalls — `virtio-blk: timeout`
— and by the modal-edge race fixed above). Board (`dev-f94c6744`, 2026-10-09): Print → the tool over the frozen 1920×1080 frame, the selection
moved and resized from a corner by dragging (`apphost: drag (…)->(…) hit=Some(8)` ×2), the mode
switch, the calculator picked in Window mode with the pointer toggle → `screencapd: saved
(kind=window w=320 h=464 …)` + the toast, the tool closed by its X (`capture freeze off`, no
file) — `board-visual: screenshot` acked with desktop, typed (the keyboard via Print) and
pointer. Not exercised on that boot (operator: "passt so"): Shift+Print / Alt+Print end to end
(host-proven: `key_facts`, the screencapd contract), ESC, and the modal / tile / clipboard rungs;
the board-visible ladder ends 45/46 — `inputd: live keyboard route on` needs an ordinary key,
and Print is a key fact.

Open findings (noted, not built here):
- vfsd `OP_COPY` reads the whole source file into the never-freeing heap (copying a
  screenshot of a few hundred KiB in the file manager can exhaust it) — needs the pulled
  write.
- A stretched cross axis overrides any explicit width (literal or bound) in a flex column —
  layout semantics, documented in the test; an `.align(start)` parent is the workaround.
- Keys inside the overlay beyond ESC (Enter/Space to shoot, S/C/W/P) and a per-surface secure
  flag for apps that show secrets unmasked are follow-ups (RFC-0095 open questions).
- **Button edges merge under load** (pre-existing, input chain): inputd pushes ONE visible state
  per HID batch — the button LEVEL at the batch's end — and windowd keeps the newest staged
  sample per frame, so a press and its release inside one batch or one frame vanish, and a
  click right after a drag's release folds into that release. Seen 2026-10-09 on the one-hart
  `usb-visible` lane while the tool painted its first frame (the guest seconds behind): the
  shutter's click became the drag's end. The injector now waits for each edge's own trace; the
  fix is edge-preserving pushes (a state per button edge in a batch) in inputd and no edge
  merge in windowd's staging.

## End-state rewrite 2026-09-09 (binding; supersedes older sections where they differ)

**Ground truth 2026-09-09:** zero code — no `screencapd` among the services, no readback
entry point in windowd, no wire module, no RFC, no consent/caps, no markers; repo-wide
`screenshot|screencap` hits only host tooling (`tools/rfb_screenshot.py`). TASK-0105 (recorder)
is blocked on this. **The readback primitive is being built by TASK-0324 P0/P6:** gpud's
one-shot `scanout_sample()` (P0, `SELFTEST: display nonblack ok`) and P6's "readback off the
scanout RT with seq acks". Screen capture MUST be a consumer of that ONE readback authority —
never a second readback path.

### Goal (end system)

One capture facade (`screencapd`: `grabDisplay` / `grabWindow` / `grabRegion` → caller VMO +
metadata) over ONE pixel-readback authority (gpud's scanout-RT readback from TASK-0324 P6),
with windowd as the geometry and secure-surface authority; consent + caps fail-closed. The
share half stays with the existing TASK-0126/0127/0128.

### Non-goals

Encoding (PNG/AVIF), gallery, share sheet, peer share, recording (0105 consumes this);
per-window re-composition (a window grab = its on-screen rect, occluders included — stated
honestly in the RFC); capture UI in windowd or screencapd.

### Invariants

- `CAPTURE_PIXELS_MAX = 2_304_000` (1920×1200), `CAPTURE_BYTES_MAX = 16 + pixels × 4`; regions
  clipped/rejected against the current mode; caller-owned VMO; header-last with the ONE
  payload-VMO header (TASK-0033 D2).
- Secure surfaces (`INTENT_FLAG_SECURE`: greeter/password fields) are never captured.
- Readback is of a REVEALED frame (`seq` from the P6 acks); no readback mid-present.
- Policy fail-closed: no cap → `DENIED` + audit; `test_reject_*` for every bound.

### Decisions

- **D1 RFC-0095 "Screen capture v1":** `nexus_wire::screencapd` (`'S','C'`): `OP_GRAB=1 {kind:
  DISPLAY|WINDOW|REGION, target_sid u64, x,y,w,h u16}` + CAP_MOVE VMO → payload header
  `{status, w, h, stride, format = BGRA8888, seq}`.
- **D2 Readback primitive = gpud `OP_READBACK = 14 (13 = `OP_REVEAL`, RFC-0093 §5)`** (`nexus-display-proto`): `{rect, seq}` +
  CAP_MOVE VMO; on GL it reads the FRONT render target (generalizing P0's `scanout_sample()`),
  on 2D it copies from windowd's scanout FB VMO. P0's one-shot sampler is REPLACED by this op
  in the same package: `SELFTEST: display nonblack ok` is re-issued through `OP_READBACK`;
  gate = `scanout_sample` deleted, the nonblack marker emitted only by the readback op's path.
- **D3 windowd hook = `OP_SURFACE_CAPTURE = 32`** (screencapd → windowd): resolves
  `WINDOW`/`REGION` to a display rect, refuses secure surfaces, forwards the moved VMO to gpud
  `OP_READBACK` — no pixels, no UI in windowd (`docs/dev/ui/windowd-cleanup-map.md`).
- **D4 Consent = policy.** Services need policyd cap `screencap.grab` (`screencapd = ["ipc.core",
  "policy.delegate"]`, selftest-client granted); apps need `nexus.permission.SCREENCAP`,
  ceiling-gated in nxb-pack to shell/settings bundle types; the consent *dialog* is a
  TASK-0074 modal in the requesting app — never in screencapd.
- **D5 Topology via the TASK-0324 P4 arm:** `ServiceId::Screencapd = 32`,
  `ServiceSpec{exposes_server, reply_inbox, routes_to: [Windowd, Policyd]}`, `REQUIRED_ROUTES +=
  (Screencapd→Windowd), (Screencapd→Policyd), (Execd→Screencapd), (SelftestClient→Screencapd)`,
  volume-shipped, background affinity.

### Packages

- **P0** RFC-0095 (RFC-0093's seq contract is in place — TASK-0324 Done). **Verified 2026-10-06: no `OP_READBACK` and no `scanout_sample` exist (only `OP_REVEAL = 13`; the lanes' pixel proofs are host screendumps) — P1 BUILDS the one readback authority (dc: a CPU copy of the scanout block; virtio 2D: the scanout VMO; GL: the front RT); nothing to delete.** `ServiceId::Screencapd` = the next free id at P0. Blast: paper.
- **P1** gpud `OP_READBACK` + host fixture (checkerboard RT → rect checksum; oversize / OOB
  reject) + `scanout_sample` deletion. Blast: display lanes (visible, gpu-pci), `gpud: chain G*`,
  the nonblack marker.
- **P2** windowd `OP_SURFACE_CAPTURE` + secure refusal (`windowd/tests/capture_gate.rs`).
  Blast: windowd host, smp1.
- **P3** screencapd service + tests + topology/policy + markers. Blast: volume boot, policy
  lanes, visible lane.
- **P4** Docs.

### Definition of Done

Host: checksum equality display/window/region; `test_reject_oob`, `test_reject_over_cap`,
`test_reject_secure`, `test_reject_no_cap`. QEMU (registered in `proof-manifest/markers/
ui.toml`, `scripts/qemu-test.sh`, `markers.txt` via the gpud/windowd contracts):
`screencapd: ready`, `screencapd: grab ok (kind=display w=1280 h=800 bytes=…)`, `screencapd:
grab deny (reason=policy)`, `SELFTEST: ui v7 screencap ok` (selftest grabs the display,
verifies header + non-black checksum), `SELFTEST: display nonblack ok` retained via the new
path. Docs: `docs/dev/ui/system-experiences/capture-and-share/screencap-share.md`,
`docs/testing/os-markers.md`.

### Touched paths

`source/services/screencapd/**`, `source/libs/nexus-wire/src/screencapd.rs`,
`source/libs/nexus-display-proto/src/{lib.rs,surface_capture.rs}` (approval zone),
`source/drivers/gpud/src/{service.rs,gl_scanout.rs,backend/present.rs}`,
`source/services/windowd/src/compositor/runtime/capture.rs`, topology/policy/volume lists,
markers triple.

### Dependencies

TASK-0324 P6 (readback off the scanout RT, seq acks) — hard; TASK-0074 (consent modal);
TASK-0033 D2 (payload header); TASK-0054C (`call` API).

## Rebase (2026-08-14) — capture-only — historical, superseded by the end-state rewrite above

### Verified reality

**Zero code exists.** A grep for `screencap|screenshot` across `source/`,
`userspace/`, and `tools/` finds only host-side QEMU proof tooling. Everything
in this ledger is greenfield — nothing to re-implement, but nothing to lean on
either.

### Scope cut — the share half moves out

The Non-Goals already point there: the intent-based share pipeline is
TASK-0126/0127/0128. This rebase completes the cut:

- **`sharesheetd` is removed from this ledger** (it was "name TBD" anyway).
- The share-sheet UI is a **DSL app surface** (in the mold of settings'
  `userspace/apps/settings/ui/components/chrome/PickerSheet.nx`), NOT a
  "SystemUI overlay". Per the boundary SSOT
  (`docs/dev/ui/windowd-cleanup-map.md:4-9`), shell UI belongs to the DSL
  shell app and widgets — never windowd, never a bespoke overlay service.

What remains here is the **capture substrate**:

1. `screencapd` service: `grabDisplay` / `grabWindow` / `grabRegion` →
   VMO + metadata (w/h/stride).
2. A bounded **windowd readback API** that screencapd consumes — windowd
   exposes readback of the last composed buffer and nothing more: no capture
   UI, no consent UI, no export logic in windowd.
3. **Consent model + pixel/byte caps** (fail-closed, policy-guarded via
   policyd).

### Dependency kept

TASK-0105 (screen recorder / capture overlay) depends on this capture
substrate.

### Process gate

**RFC seed required** for the screencap/readback API (new service API + wire
format) before implementation: `docs/rfcs/RFC-TEMPLATE.md`, next free number,
RFC index update. New markers must land together with `scripts/qemu-test.sh`
and `tools/nx/chains/markers.txt` (no-fake-green contract).

### Corrected proof + touched paths

`tests/ui_v7c_host/` never existed; host proofs go to
`source/services/screencapd/tests/` (service layout: src/ + tests/) and
`source/services/windowd/tests/` for the readback fixture. Allowlist below
updated; share-broker/export/sheet bullets in the sections that follow are
superseded by this rebase.

## Context — historical, superseded by the end-state rewrite above

Screenshot and sharing are powerful and privacy-sensitive. With kernel unchanged, the capture pipeline
must be implemented in userspace, most naturally by `windowd` readback of the last composed buffer
and a dedicated service facade (`screencapd`).

We also need a minimal share-sheet broker to route payloads to:

- clipboard,
- save-to-file under `/state`,
- (optional) peer via DSoftBus (stubbed by default).

## Goal (rebased 2026-08-14 — capture-only) — historical, superseded by the end-state rewrite above

Deliver:

1. `screencapd` service:
   - `grabDisplay`, `grabWindow`, `grabRegion`
   - returns VMO + metadata (w/h/stride)
   - implemented via a bounded `windowd` readback API
2. Privacy/policy:
   - consent model for screencap (v1: allow in selftests only; otherwise require explicit “consent” flag from focused window)
   - size/pixel caps; reject out-of-bounds regions
3. Host tests + OS markers.

## Non-Goals

- Kernel changes.
- Full gallery app.
- Any peer share.
- **The share half entirely** (broker, export destinations, sheet UI): the
  intent-based share pipeline is Share v2 (`TASK-0126`/`TASK-0127`/`TASK-0128`);
  the sheet UI is a DSL app surface there. `sharesheetd` is cut from this ledger.

## Constraints / invariants (hard requirements)

- Bounded capture:
  - cap max pixels and max bytes per capture
  - reject out-of-bounds regions
- Deterministic output for test patterns (host tests).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`source/services/screencapd/tests/` + `source/services/windowd/tests/`
(corrected 2026-08-14; `tests/ui_v7c_host/` never existed):

- render a checkerboard into a composed buffer fixture
- `grabRegion` returns correct checksum (display/window/region variants)
- out-of-bounds region and over-cap pixel/byte requests reject deterministically
- policy deny case blocks capture without consent flag (host simulated)

### Proof (OS/QEMU) — gated

UART markers (new markers land together with `scripts/qemu-test.sh` +
`tools/nx/chains/markers.txt`):

- `screencapd: ready`
- `screencap: grab ok (kind=display|window|region)`
- `SELFTEST: ui v7 screencap ok`

## Touched paths (allowlist) — corrected 2026-08-14

- `source/services/screencapd/` (new: src/ + tests/, os-lite)
- `source/libs/nexus-wire/src/` (screencapd wire module — approval zone `source/libs/**`)
- `source/services/windowd/` (readback API only; check the cleanup map first)
- `docs/rfcs/` (screencap/readback RFC seed — approval zone)
- `source/apps/selftest-client/`
- `docs/dev/ui/system-experiences/capture-and-share/screencap-share.md`

## Plan (small PRs)

1. RFC seed for the screencap/readback API (approval gate)
2. windowd readback API + bounds/limits + host fixture
3. screencapd (grab APIs, consent + caps, policy deny) + markers
4. tests + OS markers + docs

## Follow-ups

- Share v2 (intent-based, multi-app): `TASK-0126` (intentsd+policy), `TASK-0127` (chooser+targets+grants), `TASK-0128` (app senders+selftests+docs)
