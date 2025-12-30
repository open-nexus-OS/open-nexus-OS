---
title: TASK-0193 Networking v1a (host-first): devnet flag + resolverd hosts + rustls TLS validate/pinning + fetchd real backend + tlstestd + deterministic tests/docs
status: Draft
owner: @networking
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - WebView Net v1 fetch path (fetchd/httpstubd): tasks/TASK-0177-webview-net-v1b-os-httpstubd-fetchd-downloadd-policy-selftests.md
  - Trust store unification (keys/roots): tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Network basics offline daemons (hosts/DNS direction): tasks/TASK-0138-network-basics-v1a-offline-controlplane-daemons.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We currently keep OS/QEMU offline and deterministic. We still want a **real HTTPS path** for development and
integration testing, but only behind a **developer flag** (“devnet”) and with a deterministic fixture server.

This task is host-first and does not require OS networking to work. OS enablement is handled in `TASK-0194`.

## Goal

Deliver:

1. Policy/config surface (host-first):
   - caps:
     - `net.dev.enable`, `net.tcp.connect`, `net.dns.resolve`, `net.tls.validate`, `net.trust.manage`
   - config schema includes:
     - `network.devnet.enabled=false` by default
     - deterministic hosts map (no external DNS)
     - HTTP timeout and max body bytes
2. `resolverd` (hosts-only resolver):
   - resolves from config hosts map only (no external queries)
   - deterministic list/set behavior (set gated by devnet)
3. TLS client service (host-first) using rustls:
   - trust roots from `pkg://trust/roots-dev.pem` plus optional dev-installed roots
   - pinning support (SPKI sha256) for selected hosts
   - deterministic validation policy:
     - **dev-only**: allow ignoring NotBefore/NotAfter due to clocklessness (must be explicit and gated by devnet)
     - hostname/SNI validation is still required
4. `fetchd` integration:
   - keep existing fixture routing (`http://fixture.local/* → httpstubd`)
   - add real backends:
     - `http://host:port` and `https://host:port` only when `devnet.enabled=true`
   - enforce:
     - caps checks
     - response size cap
     - redirect policy (same-host only)
5. TLS fixture server `tlstestd` (host app):
   - deterministic TLS server on 127.0.0.1 with static cert chain
   - fixed responses from `pkg://fixtures/tls/www/`
6. Host tests (`tests/networking_v1_host/`):
   - devnet off denies real fetch deterministically
   - devnet on succeeds against tlstestd
   - pinning ok/fail deterministic
   - redirect same-host ok, cross-host denied
   - body size cap enforced
7. Docs:
   - devnet model and determinism guardrails
   - trust roots/pinning and fixture cert generation notes

## Non-Goals

- OS/QEMU real networking.
- External DNS or Internet access.

## Constraints / invariants (hard requirements)

- Offline by default: devnet disabled unless explicitly enabled.
- Deterministic tests (fixture server + fixed seeds; no wallclock dependency).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (clockless TLS is dev-only)**:
  - Ignoring certificate time validity is insecure. This must be gated behind `net.dev.enable` and clearly documented as dev-only.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p networking_v1_host -- --nocapture`

## Touched paths (allowlist)

- `source/services/fetchd/` (extend backend selection)
- `source/services/resolverd/` (new)
- `source/services/tlsd/` (new, host-first)
- `source/services/netd/` (new, host-first)
- `userspace/apps/tlstestd/` (new; host app)
- `tools/nx-net/` (new; host tool)
- `tests/networking_v1_host/`
- `pkg://trust/` + `pkg://fixtures/tls/`
- `docs/networking/`

## Plan (small PRs)

1. resolverd + config schema + host tests
2. tlstestd + dev roots fixtures
3. tlsd/netd host-first + fetchd backend selection + tests
4. docs + nx-net helper

## Acceptance criteria (behavioral)

- Host tests deterministically prove devnet gating, TLS verification/pinning, redirect policy, and size caps.

