---
title: TASK-0195 DSoftBus v1.1a (host-first): Noise-secured channels + encrypted framed streams + file-share protocol (quota/resume) + deterministic tests
status: Draft
owner: @runtime
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSoftBus v1 localSim baseline (no crypto): tasks/TASK-0157-dsoftbus-v1a-local-sim-pairing-streams-host.md
  - DSoftBus v1 OS wiring + share demo: tasks/TASK-0158-dsoftbus-v1b-os-consent-policy-registry-share-demo-cli-selftests.md
  - Keystore v1.1 (Ed25519 identity + seal/unseal): tasks/TASK-0159-identity-keystore-v1_1-host-keystored-lifecycle-nonexportable.md
  - Trust store unification (allowlists/roots): tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Networking devnet (host-first TLS, gating): tasks/TASK-0193-networking-v1a-host-devnet-tls-fetchd-integration.md
  - Testing contract: scripts/qemu-test.sh
---

## Short description

- **Scope**: Add host-first secure channels (Noise + identity proofs), encrypted framing, and resumable file-share contract.
- **Deliver**: Deterministic crypto/error-path tests (allowlist deny, tamper reject, quota/resume checks).
- **Out of scope**: UDP discovery transport and kernel-side crypto changes.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock secure-channel, encrypted framing, and resume/quota invariants.
- **Phase B (Host proof)**: requirement-named host tests (including reject/tamper paths) are green.
- **Phase C (OS-gated proof)**: OS claims remain gated until corresponding OS secure-path evidence exists.
- **Phase D (Performance gate)**: bounded overhead/backpressure budgets are measured with deterministic workloads.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host: `cargo test -p dsoftbus_v1_1_host -- --nocapture`
- OS (if touched): `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

DSoftBus v1 localSim (`TASK-0157`) is intentionally offline and does not include Noise/TLS crypto.
We now want a deterministic, QEMU-friendly secure-channel layer that can run host-first and later be
reused by network transports:

- Noise-secured channels (Noise_XX over X25519) authenticated with Ed25519 device identity,
- encrypted framed streams (AEAD-protected),
- and a small file-share protocol with quotas and resume rules.

This task is host-first and can run entirely over an in-process/loopback transport.
UDP devnet discovery is a separate follow-up (`TASK-0196`).

## Goal

Deliver:

1. `userspace/libs/dsoftbus-crypto` (host-first):
   - device identity:
     - Ed25519 keypair via keystored (purpose `device-id`) or deterministic fixture keys in host tests
     - fingerprint = sha256(pk) (Base32 or hex; pick one and keep stable)
   - Noise_XX handshake:
     - X25519, HKDF-SHA256, XChaCha20-Poly1305
     - include Ed25519 identity proof inside the handshake transcript
   - trust allowlist:
     - only allow peers in a deterministic allowlist file (fixture JSON) or trust store adapter
     - deny otherwise with stable error
2. `userspace/libs/dsoftbus-transport` (host-first):
   - framed, reliable stream abstraction over a `Pipe` trait (loopback adapter first)
   - encrypted framing mode:
     - AEAD per frame with monotonically increasing sequence numbers as AAD
     - crc32c is optional but must be deterministic if used
   - bounds:
     - `max_frame_bytes`, `max_inflight_frames`
3. File-share protocol `share@1` (host-first core):
   - offer/accept, chunked transfer, sha256 verification
   - resume semantics:
     - resume allowed only when `off == current_len`
     - otherwise deterministic reject
   - quota enforcement (soft/hard) with stable errors (EDQUOT)
4. Deterministic host tests (`tests/dsoftbus_v1_1_host/`):
   - handshake ok with allowlisted peer; deny with unknown peer
   - encrypted stream roundtrip equals plaintext payload
   - tamper detection: flip byte → decrypt/auth fail deterministically
   - file share: send/recv + sha256 ok
   - resume: interrupt + resume at correct offset ok; wrong offset denied
   - quota: oversized denied

## Non-Goals

- Kernel changes.
- UDP discovery/transport in this task (handled in v1.1b).

## Constraints / invariants (hard requirements)

- Deterministic tests: seeded RNG and injected clock only in tests.
- No fake security: fixture keys must be explicitly test-only; OS security claims depend on real entropy (`TASK-0159/0160` red flags).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (no_std viability later)**:
  - Keep the crypto/transport core structured so it can later be moved toward `no_std+alloc` (see `TASK-0022` direction).

## Security considerations

### Threat model

- MITM or impersonation during Noise handshake and identity proof exchange.
- Ciphertext tampering/replay against framed encrypted streams.
- File-share abuse via wrong resume offsets or quota bypass.

### Security invariants (MUST hold)

- Unknown peers are denied by trust allowlist.
- AEAD integrity failures are fail-closed and do not leak plaintext.
- Resume is accepted only at exact expected offset; quota limits are enforced deterministically.

### DON'T DO (explicit prohibitions)

- DON'T use fixture keys outside explicitly test-only contexts.
- DON'T continue transfer on auth/tag mismatch.
- DON'T accept out-of-order resume that changes prior committed bytes.

### Attack surface impact

- Significant: crypto/session boundary and remote file-transfer path.

### Mitigations

- Noise transcript identity proof, allowlist gating, monotonic sequence/AAD checks, and strict quota/resume validation.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus_v1_1_host -- --nocapture`
- Required tests:
  - `test_reject_unknown_peer_allowlist`
  - `test_reject_tampered_encrypted_frame`
  - `test_reject_resume_wrong_offset`
  - `test_reject_quota_exceeded`

### Hardening markers (QEMU, if applicable)

- `dsoftbusd: auth ok`
- `dsoftbusd: remote proxy denied (unauthenticated)`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_v1_1_host -- --nocapture`

## Touched paths (allowlist)

- `userspace/libs/dsoftbus-crypto/` (new)
- `userspace/libs/dsoftbus-transport/` (new or refactor existing localSim transport)
- `tests/dsoftbus_v1_1_host/` (new)
- `docs/dsoftbus/crypto.md` (or extend overview later)

## Plan (small PRs)

1. crypto + trust allowlist + host tests
2. encrypted framing + host tests
3. file-share protocol + quota/resume + host tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove secure handshake, encrypted streams, and file share with resume/quota rules.

Follow-up:

- DSoftBus v1.1 directory + rpc multiplexing + keepalive/health and additional flow-control integration is tracked as `TASK-0211`/`TASK-0212`.
