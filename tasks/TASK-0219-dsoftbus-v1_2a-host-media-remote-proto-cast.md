---
title: TASK-0219 DSoftBus v1.2a (host-first): Media Remote (media.remote@1) protocol + remote control + transfer/group state sync + share@1 fallback + deterministic tests
status: Draft
owner: @media
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSoftBus secure channels + share@1 resume: tasks/TASK-0195-dsoftbus-v1_1a-host-secure-channels-encrypted-streams-share.md
  - DSoftBus directory + rpcmux + health: tasks/TASK-0211-dsoftbus-v1_1c-host-busdir-rpcmux-health-flow.md
  - Media UX v2.1 (audiod + mediasessd integration): tasks/TASK-0217-media-v2_1a-host-audiod-deterministic-graph-mixer.md
  - Media UX v2 semantics (playerctl/handoff): tasks/TASK-0184-media-ux-v2a-host-handoff-playerctl-deterministic-clock.md
  - Testing contract: scripts/qemu-test.sh
---

## Short description

- **Scope**: Define host-first media remote protocol and cast/group control flows over DSoftBus control plane.
- **Deliver**: Deterministic transfer/group-sync behavior with explicit share fallback and bounded drift semantics.
- **Out of scope**: Remote audio streaming transport and UDP discovery expansion.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock `media.remote@1` protocol semantics and fallback boundaries.
- **Phase B (Host proof)**: requirement-named host tests for transfer/group/reject paths are green.
- **Phase C (OS-gated proof)**: OS claims stay gated until matching OS wiring evidence exists.
- **Phase D (Performance gate)**: deterministic drift/latency budgets are defined and validated.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host: `cargo test -p dsoftbus_media_remote_v1_2_host -- --nocapture`
- OS (if touched): `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

We want deterministic “cast/remote control” over DSoftBus while staying offline and QEMU-tolerant:

- discover peers that expose `media.remote@1`,
- remote-control their media sessions,
- transfer playback control to a peer (audio remains local on each node),
- optionally offer a file via `share@1` and then instruct the peer to play it,
- support simple group sessions where play/pause/seek state is synchronized with bounded drift using deterministic clocks.

This task is host-first and uses loopback/localSim style transports; UDP remains devnet-gated and out of scope here.

## Goal

Deliver:

1. Cap’n Proto IDL for `media.remote@1`:
   - `hello/query/control/loadUri/offerOk/ping`
   - naming alignment:
     - use existing `mediasessd` service name (repo uses `mediasessd`, not `mediasessiond`)
   - capabilities list and stable error mapping
2. `mediaremoted` host-first service:
   - bridges DSoftBus rpcmux calls to local `mediasessd` + `audiod` control surfaces
   - publishes itself via busdir with deterministic metadata
3. `mediacast` library (host-first):
   - `Transfer`:
     - attempt `loadUri(pkg://...)` on peer when resolvable
     - fallback: `share@1` offer → wait → `offerOk` → play
     - local pauses only after remote confirms `playing`
   - `GroupSolo`:
     - mirror play/pause/seek deterministically
   - `GroupParty`:
     - drift-bounded sync:
       - use audiod block clocks (10ms blocks) and exchange block indices
       - align to `now + N blocks`
       - resync if drift > threshold with bounded seek steps
     - NOTE: this is control-plane sync only; no shared audio buffer
4. Deterministic host tests `tests/dsoftbus_media_remote_v1_2_host/`:
   - directory discovery of `media.remote@1` peers via busdir
   - remote load/play and query state
   - transfer: A stops after B starts; position monotonic in blocks
   - group solo: pause/seek mirrored
   - group party: injected delay triggers resync; drift bounded
   - share fallback: offer+sha256 ok, then play

## Non-Goals

- Kernel changes.
- UDP discovery/transport (devnet gated tasks).
- Streaming remote audio output (explicitly not in v1.2).

## Constraints / invariants (hard requirements)

- Determinism:
  - injected monotonic clocks in tests
  - stable ordering of peers and sessions
  - stable drift calculation and resync policy
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success:
  - “transfer ok” only when remote reports playing and local is paused/stopped.

## Red flags / decision points (track explicitly)

- **YELLOW (group sync semantics)**:
  - “party mode” must define a strict, deterministic model (block-aligned) and avoid wallclock/NTP claims.

## Security considerations

### Threat model

- Unauthorized remote media control commands.
- Malicious transfer/group commands causing inconsistent playback state.
- Share fallback misuse to inject unexpected payloads.

### Security invariants (MUST hold)

- `media.remote@1` control paths require authenticated, authorized peers.
- Transfer completion requires remote playing confirmation before local stop/pause.
- Share fallback content handling is bounded and verified (hash/size checks).

### DON'T DO (explicit prohibitions)

- DON'T treat media control channel as trusted without rpcmux/session auth.
- DON'T claim group-sync success without deterministic drift-bound assertions.
- DON'T bypass share integrity checks for playback convenience.

### Attack surface impact

- Significant: remote control over media session behavior.

### Mitigations

- authenticated rpcmux calls, deterministic state-transition checks, bounded sync and fallback rules.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus_media_remote_v1_2_host -- --nocapture`
- Required tests:
  - `test_reject_media_control_without_cap`
  - `test_reject_transfer_without_remote_confirm_playing`
  - `test_reject_share_fallback_hash_mismatch`

### Hardening markers (QEMU, if applicable)

- `SELFTEST: media cast transfer ok`
- `SELFTEST: media cast party sync ok`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_media_remote_v1_2_host -- --nocapture`

## Touched paths (allowlist)

- `source/services/mediaremoted/` (new)
- `userspace/libs/mediacast/` (new)
- `tools/nexus-idl/schemas/media_remote.capnp` (or canonical schema location)
- `tests/dsoftbus_media_remote_v1_2_host/` (new)
- docs may land in v1.2b

## Plan (small PRs)

1. IDL + mediaremoted bridge + deterministic fixtures
2. mediacast transfer + share fallback + tests
3. group solo/party sync + drift bounds + tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove media remote discovery/control, transfer, group sync, and share fallback.
