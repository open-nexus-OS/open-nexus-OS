---
title: TASK-0158 DSoftBus v1b (OS/QEMU): perms consent + policy caps + registry persistence + Share Demo + nx-bus + selftests/postflight/docs
status: Draft
owner: @runtime
created: 2025-12-26
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusNet SDK track (cloud + DSoftBus): tasks/TRACK-NEXUSNET-SDK.md
  - DSoftBus v1a core: tasks/TASK-0157-dsoftbus-v1a-local-sim-pairing-streams-host.md
  - Permissions baseline: tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - Policy caps (capability matrix): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Clipboard v2 substrate (optional demo): tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - Share v2 (intents) later: tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Short description

- **Scope**: Wire localSim DSoftBus into OS with consent, capability policy, registry persistence gates, and demo/CLI paths.
- **Deliver**: Deterministic OS selftests and explicit stub markers when `/state` or gated features are unavailable.
- **Out of scope**: Real networked DSoftBus and full production share-intent system.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock consent/policy/persistence semantics with explicit gated behavior.
- **Phase B (Host proof)**: requirement-named host tests for consent/capability rejects are green.
- **Phase C (OS-gated proof)**: canonical QEMU marker ladder is green with honest stub behavior where required.
- **Phase D (Performance gate)**: bounded timeout/backpressure behavior validated under deterministic runs.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host: `cargo test -p dsoftbus_v1_host -- --nocapture`
- OS: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

With a deterministic localSim DSoftBus core (v1a), we need an OS-facing slice:

- pairing consent flow (permsd/SystemUI),
- policy gating for discover/pair/open-stream,
- persistent registry in `/state` when available,
- a tiny share demo app (text/file over streams),
- CLI, OS selftests, docs, and a delegating postflight.

This remains offline and deterministic: no network sockets, no crypto handshake. Ready-gate uses a logd query for `dsoftbusd: ready` before selftests.

## Goal

Deliver:

1. Consent integration:
   - `pairAccept` requires a permsd-granted consent decision
   - QEMU selftests use an explicit “auto-consent test mode” flag (must be clearly labeled as such)
   - markers:
     - `perms: dsoftbus pair consent granted`
2. Policy caps:
   - enforce:
     - `bus.discover`
     - `bus.pair`
     - `bus.stream.open`
   - system apps (SystemUI + share-demo) are pre-granted in the test profile
   - marker: `policy: dsoftbus caps enforced`
3. Device registry persistence (OS-gated):
   - path: `state:/dsoftbus/peers.nxs` (Cap'n Proto snapshot; canonical)
   - optional derived/debug view: `nx bus peers --json` emits deterministic JSON
   - if `/state` is unavailable: registry is RAM-only and must emit explicit `stub/placeholder` markers (never “persist ok”)
4. Share Demo app:
   - deterministic UI flow (pick peer, pair if needed, send text / send small file / ping-pong)
   - uses:
     - MsgStream channel `share/text` for UTF-8
     - ByteStream channel `share/bytes` for file transfer
   - writes received items to `state:/share-demo/inbox/` when `/state` exists; otherwise to a deterministic RAM inbox and emits `stub/placeholder`
   - markers:
     - `share-demo: paired peer=<id>`
     - `share-demo: send text bytes=<n>`
     - `share-demo: recv file name=<name> bytes=<n>`
5. CLI `nx bus`:
   - discover/pair/paired/msg send+recv/byte send+recv
   - stable output + markers like `nx: bus discover n=<n>`
6. OS selftests (bounded, QEMU-safe):
   - `SELFTEST: dsoftbus pair ok`
   - `SELFTEST: dsoftbus msg ok`
   - `SELFTEST: dsoftbus byte ok` (if `/state` unavailable, must be explicitly skipped with `stub/placeholder` marker; never “ok”)
7. Docs + postflight:
   - docs: overview, integration rules, size limits, error semantics, demo usage
   - postflight delegates:
     - host tests (`dsoftbus_v1_host`)
     - QEMU marker contract (`scripts/qemu-test.sh`)
8. **Ready RPC (sauber gate)**:
   - add a lightweight `Ready()`/`Ping()` IPC in `dsoftbusd` and use it in selftests as the primary readiness gate (replacing logd-marker polling for this service).

## Non-Goals

- Kernel changes.
- Real network discovery/auth/streams.
- Cross-device security model beyond local pairing code (Noise/TLS comes later in networking DSoftBus tasks).
- Full Share v2 intent plumbing (share-demo is a demo app, not the final share system).

## Constraints / invariants (hard requirements)

- Deterministic behavior and bounded timeouts.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success markers (esp. around persistence and auto-consent).

## Red flags / decision points (track explicitly)

- **RED (`/state` gating)**:
  - persistent device registry and received file inbox are gated on `TASK-0009`.
  - until then: explicit non-persistent behavior; selftests must not claim persistence.

- **YELLOW (permsd availability)**:
  - if `permsd` is not yet present, consent must be explicitly stubbed and marked (never “granted”).

## Security considerations

### Threat model

- Consent bypass through test-mode or missing permsd wiring.
- Unauthorized stream and share actions without required caps.
- Persistence misuse when `/state` is unavailable.

### Security invariants (MUST hold)

- Pair accept requires explicit consent decision path.
- Capability checks (`bus.discover`, `bus.pair`, `bus.stream.open`) are enforced fail-closed.
- Persistence claims are honest: no persist-ok marker without real `/state` backing.

### DON'T DO (explicit prohibitions)

- DON'T silently auto-grant consent outside explicit test mode.
- DON'T bypass capability gates for demo convenience.
- DON'T mark persistence/selftests green when only stub behavior exists.

### Attack surface impact

- Significant: policy/consent and data-persistence surfaces in OS context.

### Mitigations

- Explicit consent flags, deny-by-default capability checks, and deterministic stub markers.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus_v1_host -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
- Required tests:
  - `test_reject_pair_accept_without_consent`
  - `test_reject_stream_open_without_caps`
  - `test_reject_persistence_claim_without_state`

### Hardening markers (QEMU, if applicable)

- `perms: dsoftbus pair consent granted`
- `policy: dsoftbus caps enforced`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_v1_host -- --nocapture`

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `dsoftbusd: ready` (ready gate: logd query in selftest-client)
    - `SELFTEST: dsoftbus pair ok`
    - `SELFTEST: dsoftbus msg ok`
    - `SELFTEST: dsoftbus byte ok` (only when `/state` exists; otherwise explicit `stub/placeholder`)

## Touched paths (allowlist)

- `source/services/dsoftbusd/`
- `userspace/apps/share-demo/` (new)
- `tools/nx-bus/` (new)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh` (marker contract update)
- `tools/postflight-dsoftbus-v1.sh` (delegates)
- `docs/dsoftbus/overview.md` + `docs/dsoftbus/integration.md`
- `docs/apps/share-demo.md`

## Plan (small PRs)

1. perms/policy hooks + explicit test-mode auto-consent (clearly labeled)
2. share-demo app + nx-bus CLI
3. OS selftests + marker contract + docs + postflight

## Acceptance criteria (behavioral)

- In QEMU, localSim pairing + msg stream roundtrip + byte stream transfer are proven by selftest markers.
- Any missing dependencies (`/state`, `permsd`) are handled explicitly with `stub/placeholder` behavior and do not produce “ok” markers.
- Readiness gating is enforced before selftests: the logd query gate must observe `dsoftbusd: ready` prior to dsoftbus checks.
