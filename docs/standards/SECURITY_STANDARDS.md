---
schema: 1
title: "Security standards"
role: reference
subsystem: devx
sources: [source/services/policyd, source/services/keystored, source/apps/selftest-client/proof-manifest/markers/policy.toml, source/kernel/neuron/src/cap]
contracts: [RFC-0005, RFC-0008, RFC-0075, RFC-0091]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Security standards

> **Purpose** — the security invariants every change keeps, the tests and markers that prove them, and the review checklist.
> **Owns** — no code; enforced by review, `test_reject_*` tests and the proof manifest
> **Contracts** — [RFC-0005](../rfcs/RFC-0005-kernel-ipc-capability-model.md) (capabilities), [RFC-0008](../rfcs/RFC-0008-dsoftbus-noise-xk-v1.md) (identity binding), [RFC-0075](../rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md) (typed text delivery), [RFC-0091](../rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md) (ABI profiles)
> **Invariants** — identity is the kernel's `sender_service_id`; sensitive operations go through `policyd`, deny by default; no secret and no keystroke rhythm reaches a log.
> **Read next** — [RUST_STANDARDS.md](RUST_STANDARDS.md), [../security/policyd.md](../security/policyd.md), [../security/capabilities.md](../security/capabilities.md)

## Principles

1. **Minimal kernel** — policy, crypto and complex parsing stay in userspace.
2. **Capabilities** — no ambient authority; every access needs an explicit capability.
3. **Deny by default** — an operation without an explicit allow is refused.
4. **One authority** — security decisions go through `policyd`; no bypass path.
5. **Channel-bound identity** — trust the kernel-provided `sender_service_id`, never payload strings.
6. **Audit** — security-relevant decisions produce audit records.

## Invariants

### Secrets

- Secrets live in `keystored` (persisted under `/state/keystore/` through statefsd). Signing APIs
  return signatures, never keys.
- Never log plaintext secrets, private keys, credentials or session keys — not in error messages,
  not on the UART.
- Deterministic keys only in bring-up and test code, labelled
  `// SECURITY: bring-up test keys, NOT production custody`, cfg-gated out of production paths and
  named in the task with their migration.

### What the user types

Typed text travels only through imed's focus-gated delivery (RFC-0075). No log line fires per key
press, per on-screen-keyboard tap, or per frame or service call a keystroke causes: a line per
keystroke writes the length and rhythm of what was typed — a password included — into the boot
log. Interaction proofs fire once per process; counts log on a new high or on powers of two
(`snapshot_due` in `source/services/metricsd/src/lib.rs`); pointer traces stay silent in the
keyboard overlay. Aggregates (rates, totals) are fine. Example:
`source/services/app-host/src/proof_line.rs`.

### Identity and authentication

- Use `sender_service_id` from kernel IPC; it cannot be forged.
- Verify identity binding (device id ↔ Noise static key) and finish authentication before
  processing application data. A mismatch rejects the session — no "warn and continue", no
  exception for localhost or "trusted" networks.

### Input

- Bound every size before parsing or allocating; validate format first.
- Reject malformed input deterministically, never crash (a crash is a DoS).
- No `unwrap`/`expect` on untrusted input. Error types that carry a security decision are
  `#[must_use]`:

```rust
#[must_use = "permission errors must be handled"]
pub enum AuthError {
    PermissionDenied,
    InvalidCredential,
    SessionExpired,
}
```

### Capabilities and policy

- Sensitive operations go through `policyd`; policy logic is not duplicated in services.
- No ambient capabilities; grant the minimum. No policy skip for "trusted" services.
- Policy does not change at runtime, with one exception (RFC-0091 §6): the per-subject ABI mode
  (Enforce ↔ Learn) through `OP_SET_ABI_MODE` in `source/services/policyd/src/abi_mode.rs` —
  sender-authenticated (`policy.abi_mode`), epoch-guarded, audited, never persisted (every boot
  starts in Enforce). Profiles never change at runtime; Learn mode never changes a decision.

### Memory and devices

- W^X: the kernel refuses a writable and executable mapping.
- MMIO maps are USER|RW, never EXEC, bounded to the device capability's window; device
  capabilities are minted by init from the device tree, not granted to arbitrary services.
- VMO sizes and offsets are validated; a bus address leaves the kernel only through `vmo_runs` to
  the device's holder.

## Security-relevant work

A task is security-relevant when it touches crypto or authentication, the network, IPC or
capabilities, the kernel boundary (syscalls, MMIO, mappings), secrets or personal data, signing and
updates, or key and credential storage. UI layout and docs-only changes are not. Such a task states
its threat model, invariants and attack surface, and its proof carries `test_reject_*` tests and
hardening markers (`tasks/TASK-TEMPLATE.md`).

## Proof

### Negative tests

Every security path has a `test_reject_*` test that proves the refusal:

```rust
#[test]
fn test_reject_oversized_input() {
    let oversized = vec![0u8; MAX_INPUT_SIZE + 1];
    assert!(parse_packet(&oversized).is_err());
}
```

```bash
cargo test -- reject --nocapture       # every reject test in the workspace
cargo test -p dsoftbusd -- reject
cargo test -p keystored -- reject
cargo test -p nexus-sel -- reject
just test-kernel                        # the kernel's own reject matrices
```

### Hardening markers

Markers that prove enforcement, declared in the proof manifest
(`source/apps/selftest-client/proof-manifest/markers/policy.toml`,
`source/apps/selftest-client/proof-manifest/markers/net.toml`):

| marker | proves |
|---|---|
| `dsoftbusd: auth ok` | handshake and identity binding succeeded |
| `dsoftbusd: identity mismatch peer=<id>` | binding enforcement rejects a wrong key |
| `dsoftbusd: announce ignored (malformed)` | discovery input validation |
| `policyd: audit emit ok` | an audit record reached logd |
| `SELFTEST: policy deny audit ok` | deny decision plus its audit record |
| `SELFTEST: policy allow audit ok` | allow decision plus its audit record |
| `SELFTEST: keystored sign denied ok` | signing refused without the capability |

A marker change updates `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt` and the manifest
together.

### Property and state-machine tests

There is no `cargo fuzz` target in the tree. Parsers and state machines are covered by property
tests (`proptest`) and bounded deterministic stress mixes:

| where | what |
|---|---|
| `source/kernel/neuron/src/cap/tests_prop.rs` | capability-table rights properties |
| `source/kernel/neuron/src/mm/frames/tests.rs` | frame allocator properties |
| `source/kernel/neuron/src/va_space_tests.rs` | VA allocator invariant after every random op |
| `source/kernel/neuron/src/ipc/mod.rs` | router state-machine stress |
| `source/services/logd/tests/journal_protocol.rs` | journal protocol decoder |
| `userspace/samgr/src/lib.rs` | registry properties |

A new parser for untrusted input adds one of these.

## Review checklist

- [ ] No payload-provided identity trusted; `sender_service_id` for every policy decision
- [ ] Identity binding verified before `auth ok`; no "warn and continue"
- [ ] No secrets in logs, markers or errors; no log line per keystroke, tap or keystroke-caused call
- [ ] Test keys labelled and out of production paths
- [ ] Every input bounded; no `unwrap`/`expect` on untrusted data; malformed input refused, not a crash
- [ ] Sensitive operations through `policyd`; deny by default; audit records produced
- [ ] No executable device mapping; MMIO bounded to the device window
- [ ] `test_reject_*` tests and hardening markers present

## Escalation

Mark a blocking security concern `RED (blocking)` in the task or RFC; tag the PR
`security-review-required`.
