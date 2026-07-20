---
title: TASK-0296 nexus-wire — declarative service frame codec + nexus-abi identity split
status: Done (2026-07-20)
owner: @runtime
created: 2026-07-20
depends-on: []
follow-up-tasks:
  - Consumer import migration nexus_abi::<svc> → nexus_wire::<svc> + shim removal
  - Consolidate userspace/nexus-ipc/{logd_wire,policyd_wire}.rs onto nexus-wire
  - abi_filter.rs adopts the codec engine
  - slot_probe.rs relocation to nexus-service-entry (optional)
  - RFC-0066 typed Connection::call rollout on top of nexus-wire
  - Retire unused nexus-idl macro crate (decision)
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - ADR: docs/adr/0051-declarative-wire-codec-nexus-wire.md
  - ADR: docs/adr/0038-display-wire-ssot-and-capnp-boundary.md
  - RFC: docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md
---

## Context

- `source/libs/nexus-abi/src/lib.rs` is the largest grandfathered structure-gate
  entry (4103 LOC). Its wire half (~1810 LOC, 9 service modules, 66
  `encode_*`/`decode_*` fns) is fully hand-coded with zero shared abstraction:
  the 3-byte magic/version guard is duplicated ~40×, `op | 0x80` and the
  `len==0||len>48` bound are hand-repeated, and every new service copies the
  "sessiond template" (TASK-0072) by hand.
- Service↔service wire framing is not kernel↔userspace ABI; it does not belong
  in nexus-abi (crate identity, ADR-0016 charter).
- *Must not change*: wire bytes (golden-byte tests are the contract), consumer
  import paths (`nexus_abi::<svc>::…` keeps resolving via re-export shims),
  the `nexus_env` cfg surface, the pinned toolchain.

## Goal

- Service wire frames are **declared once** in `source/libs/nexus-wire`
  (`frames!` DSL over a tiny codec core) and nexus-abi shrinks to its real
  charter (syscall ABI), with every produced/accepted byte identical to today.

## Non-Goals

- No Cap'n Proto migration of these frames (settled by ADR-0038).
- No consumer import migration in this pass (shim keeps paths stable).
- No RFC-0066 typed `Connection::call` rollout (separate track).
- No behavior change in abi_filter.rs / slot_probe.rs.

## Constraints / invariants (hard requirements)

- **No fake success**: no new markers; the existing marker ladder must stay
  green (byte-identical wire ⇒ identical boot behavior, proven, not assumed).
- **Byte identity**: every existing golden-byte test moves verbatim into
  nexus-wire and passes with **zero assertion edits**.
- **Fail-closed decoding**: every decoder returns `None` on bad
  magic/version/op/length/UTF-8; strict exact-length is the default.
- **Rust hygiene**: nexus-wire is `no_std`, `#![forbid(unsafe_code)]`,
  `#![deny(clippy::all, missing_docs)]`, zero deps, alloc-free.
- **Structure gate honesty**: baseline regen may only remove/shrink entries.

## Red flags / decision points (track explicitly)

- **RED**: none open.
- **YELLOW**: `frames!` const-sum `[u8; N]` return types must compile on the
  pinned nightly-2025-01-15 — verified first in the engine step before any
  migration. Trailing-byte semantics: engine defaults to exact-length; audit
  each decoder during its migration (goldens + `malformed_*` tests are the
  tripwire).
- **GREEN**: wire half is pure safe/no_std/alloc-free code; the only coupling
  into the rest of the crate is `policyd → abi_filter::MAX_PROFILE_BYTES`
  (moves to `nexus_wire::policyd`, abi_filter re-sources it).

## Security considerations

### Threat model
- Malformed/malicious IPC frames from less-trusted services (parsing untrusted
  input is this code's whole job).

### Security invariants (MUST hold)
- All frame input is bounded and validated before use; length-prefixed fields
  enforce min/max bounds in the engine, once.
- Decoders are fail-closed (`None`), never panic, never `unwrap`/`expect`.
- No identity is derived from payload strings (unchanged: identity =
  `sender_service_id` from kernel IPC).

### DON'T DO (explicit prohibitions)
- DON'T accept trailing bytes silently where today's decoder rejects them.
- DON'T "fix" a golden vector to make a migration pass.

### Attack surface impact
- Minimal: same wire surface, one shared decoder core instead of 40 hand
  copies — fewer places for a bounds bug.

### Mitigations
- Deterministic reject matrix (`test_reject_truncation_and_mutation`) per
  protocol: every 1-byte truncation and every header-byte mutation of each
  golden frame must decode to `None` (TASK-0231 spirit, no new deps).

## Security proof

### Audit tests (negative cases / attack simulation)
- Command(s):
  - `cargo test -p nexus-wire -- reject --nocapture`
- Required tests:
  - `test_reject_truncation_and_mutation` (per protocol module)
  - engine bounds-edge tests (str8/bytes8 min/max, finish_exact)

## Contract sources (single source of truth)

- **ABI/layout contract**: golden-byte tests in `source/libs/nexus-wire/src/*`
  (moved verbatim from `source/libs/nexus-abi/src/lib.rs`)
- **QEMU marker contract**: `scripts/qemu-test.sh` (unchanged; regression
  signal only)

## Stop conditions (Definition of Done)

- **Proof (tests)**:
  - `cargo test -p nexus-wire` — all moved golden tests + reject matrices green
  - `just test-host` — whole workspace green (51 dependents compile via shims)
- **Proof (gates)**:
  - `just check` green (fmt, clippy, deny, arch, structure-gate)
  - `just dep-gate` green (nexus-wire = zero-dep node)
  - `config/loc-baseline.txt` diff only removes/shrinks entries
- **Proof (QEMU)**:
  - `just ci-os-smp1` boot gate green (marker ladder unchanged)

## Touched paths (allowlist)

- `source/libs/nexus-wire/**` (new)
- `source/libs/nexus-abi/**`
- `Cargo.toml` (workspace members)
- `config/loc-baseline.txt` (regen)
- `docs/adr/0051-*.md`, `docs/adr/README.md`, `CHANGELOG.md`, `tasks/TASK-0296-*.md`

## Plan (small PRs)

1. Docs: ADR-0051 + this ledger.
2. Engine: nexus-wire crate, `codec.rs` + `frames!` + engine tests (verify
   const-sum `[u8; N]` on pinned toolchain first).
3. Pilot: settingsd + execd (both frame genera), shims in nexus-abi.
4. Wave: updated, routing, sessiond, bundleimg, policy.
5. Special cases: bundlemgrd (hand-written VMO payload part), policyd
   (v1/v2/v3, nonces, MAX_PROFILE_BYTES move).
6. Half-B split: nexus-abi `syscall/` modules, root re-exports keep paths.
7. Finish: baseline regen, CHANGELOG, ADR → Accepted, QEMU boot proof.

## Acceptance criteria (behavioral)

- `nexus_abi::<svc>::encode_*/decode_*` paths resolve unchanged for all 51
  dependents; zero consumer edits.
- All pre-existing golden-byte assertions pass unmodified in their new home.
- Every protocol decoder has a reject matrix over its golden frames.
- No file in nexus-abi/nexus-wire exceeds the 600-LOC structure gate; the
  4103 grandfather entry is gone.

## Evidence (2026-07-20)

- `cargo test -p nexus-wire`: 56 passed, 0 failed (all moved golden tests +
  per-protocol reject matrices + engine/DSL tests).
- `just test-host`: green across the workspace (591 suite results, 0 failed) —
  all 51 dependents compile via the shims with zero consumer edits.
- `just check`: fmt + clippy + deny + arch-check + structure-gate PASS.
- `just dep-gate`: "RFC-0009 dependency hygiene: no forbidden crates" PASS
  (nexus-wire is a zero-dep node).
- `just diag`: zero warnings under host + os slice + kernel cfgs.
- `config/loc-baseline.txt` regen: removes the 4103-LOC nexus-abi entry
  (largest grandfather), no entry grew. `lib.rs` now 183 LOC; largest new
  file `syscall/task.rs` 466 LOC.
- QEMU `just ci-os-smp1` (log `build/logs/smp--2026-07-20T13-52-00`):
  `SELFTEST: Completed (markers verified)`, chain-marker contract 9/9,
  `KSELFTEST: bkl budget ok`. One earlier run hit the known nondeterministic
  early-boot park (tracked separately in the boot-park debugging track — that
  run also lacked the bkl report because selftest-client never reached the
  end phase); the same binary passes on rerun, matching the flake signature,
  not a wire regression.
- Control experiment (changes stashed, clean HEAD, same afternoon): run 1
  FAILED with `Missing UART marker: KINIT: cpu1 online` (cpu1 never came
  online — the early-boot park), run 2 passed. The flake reproduces on HEAD
  without this task's changes and each failure trips a different marker; the
  boot-reliability issue is pre-existing and tracked in the boot-park
  debugging track.

## RFC seeds (for later, when the step is complete)

- Decisions made: declarative frame DSL over codec core (ADR-0051); wire
  protocols are not ABI (crate identity split).
- Open questions: when to run the consumer import migration; whether
  nexus-ipc mirrors consolidate before or after RFC-0066 typed client work.
- Stabilized contracts: golden-byte tests + reject matrices in nexus-wire.
