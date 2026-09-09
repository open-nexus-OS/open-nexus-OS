---
title: TASK-0068 UI v7c: screencapd over the ONE pixel-readback authority (gpud readback + windowd geometry/secure-surface gate) + consent/caps
status: Draft (end-state rewrite 2026-09-09; depends on TASK-0324 P6 readback; RFC seed required)
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

- **P0** RFC-0095 (after TASK-0324 P1's RFC-0093 fixes the readback/seq contract). Blast: paper.
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
