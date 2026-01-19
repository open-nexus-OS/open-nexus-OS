---
title: TRACK DAW App (Logic Pro iPad-class): low-jitter audio engine + MIDI + plugin hosting (CLAP-first, LV2 subset optional), capability-gated
status: Draft
owner: @media @ui @runtime
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority registry (names are binding): tasks/TRACK-AUTHORITY-NAMING.md
  - Keystone closure plan: tasks/TRACK-KEYSTONE-GATES.md
  - NexusMedia SDK (audio contracts, soft realtime): tasks/TRACK-NEXUSMEDIA-SDK.md
  - NexusNet SDK (optional downloads/sync, policy-gated): tasks/TRACK-NEXUSNET-SDK.md
  - NexusGfx SDK (render/compute, UI acceleration): tasks/TRACK-NEXUSGFX-SDK.md
  - Drivers & accelerators (audio device-class direction): tasks/TRACK-DRIVERS-ACCELERATORS.md
  - Zero-copy data plane (VMOs): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (soft realtime spine): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Deterministic parallelism policy: tasks/TASK-0276-parallelism-v1-deterministic-threadpools-policy-contract.md
  - Audio core (host-first): tasks/TASK-0254-audio-v0_9a-host-mixer-ringbuffer-levels-deterministic.md
  - Audio OS wiring (audiod): tasks/TASK-0255-audio-v0_9b-os-audiod-i2sd-codecd-mediasession-hooks-selftests.md
  - Media sessions/system controls: tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
  - Zero-Copy App Platform (autosave/oplog concepts): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Packages / signed bundles (format + install): tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - Packages / bundle install authority: tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Storefront UI (licensed registry direction): tasks/TASK-0181-store-v1b-os-storefront-ui-selftests-policy-docs.md
  - Content broker quotas/versions (sound packs as content): tasks/TASK-0232-content-v1_2a-host-content-quotas-versions-naming-nx-content.md
  - Content SAF flows (picker/save/open; grants): tasks/TASK-0233-content-v1_2b-os-saf-flows-files-polish-privacy-gates-selftests.md
---

## Goal (track-level)

Deliver a first-party **DAW** comparable to Logic Pro for iPad:

- multitrack audio recording + editing,
- MIDI tracks (piano roll) + automation lanes,
- mixer (buses, sends, inserts) with meters,
- instruments/effects via plugins,
- export/bounce with bounded profiles,
- touch-first UI with consistent “first-party” feel.

This app is a reference workload for:

- `tasks/TRACK-NEXUSMEDIA-SDK.md` (audio engine contracts, low jitter, policy),
- `tasks/TRACK-DRIVERS-ACCELERATORS.md` (audio device-class service path),
- `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md` (autosave/recovery discipline).

## Non-goals (avoid drift)

- Not “VST3 compatibility by default” (licensing + architecture + ecosystem mismatch; can be evaluated later).
- Not “load arbitrary desktop UIs inside the DAW”. DAW must keep a coherent UI system.
- No unbounded CPU use; realtime work must be budgeted and backpressured.

## Authority model (must match registry)

- `audiod`: single authority for audio mixing/routing and device IO (apps use SDK/service APIs)
- `policyd`: allow/deny audio record, MIDI device access, file access, plugin permissions
- `logd`: audit/log sink
- `windowd`: DAW UI/present

No parallel audio authority, no per-app “mini mixer daemon”.

## Plugin stance (ecosystem + UX, aligned with security)

### Primary plugin format: CLAP (first-class)

Rationale:

- permissive, modern API surface; good fit for a capability-first host
- encourages a clean host-owned UI story (parameter metadata + host controls)

### Secondary plugin format: LV2 subset (optional)

Rationale:

- large open-source plugin ecosystem; often portable to RISC-V
- but plugin-provided UI is not a product promise (host-owned UI remains canonical)

**Explicitly supported subset** (directional):

- audio + MIDI/event ports
- parameters + automation metadata
- state/presets where bounded and deterministic

### Future: Nexus-native plugins (first-party / partners)

Direction:

- capability-first plugin SDK (no ambient file/net/audio device)
- control plane via typed IPC; bulk buffers via VMO/filebuffer
- process isolation by default (crash containment; audited)

## Sound Library (downloadable sample / instrument packs)

The DAW ships a built-in **Sound Library** UX (Logic-style) to browse and download:

- sample packs (drums/one-shots/loops),
- instrument libraries (multisamples, presets),
- plugin preset collections (for CLAP/LV2/nexus-native plugins),

without requiring the user to manually manage files.

### Packaging + distribution stance

- Sound packs are distributed as **signed bundles** (preferred) or signed content artifacts:
  - bundle format/signing: `tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md`
  - install/upgrade authority: `tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md`
- Downloads use the **policy-gated network** path (SDK ergonomics): `tasks/TRACK-NEXUSNET-SDK.md`
- Storage uses the content broker + quotas/versioning direction (no ambient raw paths):
  - `tasks/TASK-0232-content-v1_2a-host-content-quotas-versions-naming-nx-content.md`
  - `tasks/TASK-0233-content-v1_2b-os-saf-flows-files-polish-privacy-gates-selftests.md`

### Authority + security invariants

- The DAW is a **client**. It must not become a parallel “store authority”.
- Trust decisions remain centralized:
  - signatures/installer decisions via packaging authority,
  - permissions and audit via `policyd`,
  - network calls bounded and policy-gated (no raw sockets).
- No secrets in logs:
  - licenses/entitlement tokens (if any) are never logged,
  - audits record **what was installed** (pack id + version) without embedding content.

### Boundedness requirements (hard)

- pack manifests are bounded (file counts, sizes, metadata lengths),
- extraction is bounded and cancelable,
- disk budgets enforced (quota-aware; deterministic eviction rules for caches).

## Keystone gates / blockers (what must be true for “real DAW”)

### Gate 1 — IPC + cap transfer (Keystone Gate 1)

Reference: `tasks/TRACK-KEYSTONE-GATES.md`.

Needed for: process-isolated plugin hosting and capability distribution.

### Gate 2 — Zero-copy data plane (VMO/filebuffer)

Reference: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`.

Needed for: audio buffers, sample libraries, clip caches without copy storms.

### Gate 3 — Soft realtime spine (QoS/timers + deterministic parallelism)

References:

- `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
- `tasks/TASK-0276-parallelism-v1-deterministic-threadpools-policy-contract.md`

Needed for: low jitter scheduling, predictable buffer deadlines, and deterministic test harnesses.

### Gate 4 — Audio authority is real (audiod)

References:

- `tasks/TASK-0254-audio-v0_9a-host-mixer-ringbuffer-levels-deterministic.md`
- `tasks/TASK-0255-audio-v0_9b-os-audiod-i2sd-codecd-mediasession-hooks-selftests.md`

Needed for: real capture/playback, meters, and system-consistent routing.

## Realtime safety & security invariants (non-negotiable)

- Plugins are untrusted by default: process isolation preferred; crash does not kill the DAW.
- No `unwrap/expect` on plugin-provided metadata or state blobs.
- Bounded parameter/state sizes; bounded preset parsing.
- File/network access is capability-gated; plugins do not get ambient access.
- No secrets in logs (licenses, tokens); audits record decisions, not content.

## Phase map (what “done” means by phase)

### Phase 0 — Host-first DAW core (real engine semantics, no device dependency)

- offline render engine (mix graph) using deterministic fixtures
- MIDI timing + automation model with deterministic tests
- plugin host “simulator” (in-proc) that proves scheduling + parameter automation semantics

Proof:

- host tests: “project script → audio checksum”
- negative tests: reject oversized plugin state/metadata blobs

### Phase 1 — OS/QEMU wiring (audiod + basic record/play)

- DAW uses `audiod` for playback and (if permitted) recording
- system media session controls integrate where relevant (play/stop)

Proof:

- OS markers from `audiod` selftests; DAW markers only after real record/play happened

### Phase 2 — CLAP hosting v1 (process isolation)

- isolate plugins in a worker process model (bounded IPC, shared buffers)
- host-owned UI for parameters/macros/automation

### Phase 3 — LV2 subset compatibility (optional)

- LV2 adapter for supported subset
- curated, ported plugin set for RISC-V

### Phase 4 — Nexus-native plugins (pro)

- native plugin SDK that is capability-first and deterministic by construction

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-DAW-000: Mix graph + offline render v0 (deterministic fixtures)**
- **CAND-DAW-010: MIDI + automation model v0 (bounded, deterministic)**
- **CAND-DAW-020: Plugin hosting core v0 (CLAP-first; scheduling + isolation design)**
- **CAND-DAW-030: Host-owned plugin UI system v0 (smart controls, meters, automation lanes)**
- **CAND-DAW-040: LV2 subset adapter v0 (curated; bounded parsing/state)**
- **CAND-DAW-050: Nexus-native plugin SDK v0 (capability-first IPC + zero-copy buffers)**
- **CAND-DAW-060: Sound Library v0 (browse + download + install signed packs; quota-aware)**

## Extraction rules

Candidates become real tasks only when they:

- define explicit buffer deadlines and bounded CPU budgets,
- provide host-first deterministic proofs,
- include `test_reject_*` for oversized/untrusted plugin inputs,
- keep authority boundaries (`audiod` remains the audio authority; `policyd` decides permissions).
