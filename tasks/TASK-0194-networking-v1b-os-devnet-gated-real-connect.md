---
title: TASK-0194 Networking v1b (OS/QEMU, gated): devnet-enabled TCP/TLS plumbing + fetchd real backend + trust store + selftests/docs
status: Draft
owner: @networking
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Networking step 1 (virtio-net + smoltcp): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Device MMIO access model (kernel work): tasks/TASK-0010-device-mmio-access-model.md
  - WebView Net v1 OS slice (fetchd/httpstubd): tasks/TASK-0177-webview-net-v1b-os-httpstubd-fetchd-downloadd-policy-selftests.md
  - Networking v1 host devnet slice: tasks/TASK-0193-networking-v1a-host-devnet-tls-fetchd-integration.md
  - Trust store unification: tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Persistence substrate (/state trust/custom roots): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

OS/QEMU is offline and deterministic by default. We want an optional devnet mode that enables real outbound
TCP/TLS only when:

- the OS networking stack exists (virtio-net + smoltcp; requires MMIO access work),
- devnet is explicitly enabled by config/caps,
- and trust store rules are applied.

If OS networking is not present, this task must not claim success; it must remain explicit `stub/placeholder`.

## Goal

Deliver:

1. OS devnet gating:
   - even with caps, real connect is denied unless `network.devnet.enabled=true`
   - markers:
     - `net: devnet off deny`
2. OS `fetchd` real backend enablement (gated):
   - keep `httpstubd` path as default
   - enable `http(s)` real backend only when:
     - `TASK-0003` OS sockets path exists, and
     - devnet enabled + caps allow
3. Trust store integration:
   - use canonical trust roots (`pkg://trust/` + `state:/trust/installed/`) from `TASK-0160`
   - pinning optional; certificate time validation policy must be explicit
4. OS selftests (bounded):
   - with devnet disabled:
     - attempt `https://...` fetch → denied → `SELFTEST: net dev off deny ok`
   - with devnet enabled:
     - only if OS networking exists, fetch a deterministic local endpoint and prove `SELFTEST: net https ok`
     - otherwise explicit placeholder markers (never “ok”)
5. Docs:
   - devnet security model and why it is disabled by default
   - dependency gates (MMIO + virtio-net + trust store)

## Non-Goals

- Kernel changes in this task (kernel MMIO is `TASK-0010`).
- Internet connectivity in CI.

## Constraints / invariants (hard requirements)

- No fake success: OS tests must not claim real TLS unless the OS stack truly performed it.
- Determinism: local fixture endpoints only; no external DNS.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (requires OS networking + MMIO)**:
  - Real connect requires `TASK-0003` and `TASK-0010`. Without them, OS-side must remain disabled/stubbed.

- **RED (rustls/no_std viability)**:
  - If OS userland cannot support rustls in the target environment, we must keep TLS OS-disabled and document it.
  - Host devnet (TASK-0193) still provides coverage for fetchd integration and policy semantics.

## Stop conditions (Definition of Done)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
  - Required markers (minimal):
    - `SELFTEST: net dev off deny ok`
  - Optional markers (only if unblocked):
    - `SELFTEST: net https ok`

## Touched paths (allowlist)

- `source/services/fetchd/` (OS backend gating)
- `source/apps/selftest-client/`
- `schemas/network.schema.json`
- `docs/networking/`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Wire config+caps gating and “devnet off deny” selftest
2. Add OS enablement only when OS networking exists (explicit gate/feature flag)
3. Docs and marker contract updates

## Acceptance criteria (behavioral)

- In QEMU, devnet is denied by default and proven by selftest markers; enabling devnet only works when the OS network stack exists and is explicitly gated.
