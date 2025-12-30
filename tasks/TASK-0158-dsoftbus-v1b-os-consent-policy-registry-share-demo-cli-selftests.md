---
title: TASK-0158 DSoftBus v1b (OS/QEMU): perms consent + policy caps + registry persistence + Share Demo + nx-bus + selftests/postflight/docs
status: Draft
owner: @runtime
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSoftBus v1a core: tasks/TASK-0157-dsoftbus-v1a-local-sim-pairing-streams-host.md
  - Permissions baseline: tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - Policy caps (capability matrix): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Clipboard v2 substrate (optional demo): tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - Share v2 (intents) later: tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

With a deterministic localSim DSoftBus core (v1a), we need an OS-facing slice:

- pairing consent flow (permsd/SystemUI),
- policy gating for discover/pair/open-stream,
- persistent registry in `/state` when available,
- a tiny share demo app (text/file over streams),
- CLI, OS selftests, docs, and a delegating postflight.

This remains offline and deterministic: no network sockets, no crypto handshake.

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
   - path: `state:/dsoftbus/peers.json`
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

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_v1_host -- --nocapture`

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `dsoftbusd: ready`
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

