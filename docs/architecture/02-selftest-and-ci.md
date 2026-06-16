<!-- Copyright 2024 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# NEURON Selftest Harness and Continuous Integration

Open Nexus OS follows a **host-first, QEMU-last** testing strategy.
The OS stack relies on deterministic UART markers as the canonical proof signal for QEMU smoke runs.

**Important:** this file describes the architecture of the selftest + CI flow.
The canonical marker contract is implemented by `scripts/qemu-test.sh` and documented in `docs/testing/index.md`.

## QEMU runner

`scripts/qemu-test.sh` is the canonical harness for QEMU smoke:

- boots the OS stack under QEMU headless,
- records UART output to `uart.log`,
- records diagnostics to `qemu.log`,
- and fails if required markers are missing or out-of-order.

RFC‑0014 Phase 2 adds triage helpers on top of the strict marker contract:

- **Phase-first failures**: the harness reports `first_failed_phase=<name>` alongside the missing marker.
- **Bounded excerpts**: on failure, the harness prints a bounded UART excerpt scoped to the failed phase.
- **Phase-gated early exit**: `RUN_PHASE=<name> just test-os` stops QEMU early after the requested phase and only validates markers up to that phase.

See `docs/testing/index.md` for the supported phases and examples.

Marker details drift quickly; keep them centralized in the harness and the testing guide:

- Marker contract: `scripts/qemu-test.sh`
- Testing guide (methodology + marker sequence notes): `docs/testing/index.md`

Some proofs (notably slirp/usernet DHCP and DSoftBus OS transport) can be environment-sensitive.
They remain available as explicit, opt-in requirements controlled by the QEMU smoke harness:

- `REQUIRE_QEMU_DHCP=1` — enforce DHCP lease + dependent network proofs
- `REQUIRE_DSOFTBUS=1` — enforce DSoftBus discovery/session proof markers

See `docs/adr/0025-qemu-smoke-proof-gating.md` for rationale and usage.

## Host unit tests for kernel logic (the ungated-module pattern)

The `neuron` kernel crate is built **only** for the bare-metal target: in `lib.rs` every
module is declared `#[cfg(target_os = "none")]`. A direct consequence, which is easy to
trip over:

> **`cargo test -p neuron` runs 0 tests on the host.** Because the modules are gated out for
> the host target, their in-tree `#[cfg(test)] mod tests` are never compiled or run there
> (this includes otherwise-rich suites such as `timer.rs`'s). Equally, `#[cfg(test)]` code
> inside a gated module — e.g. the `mod tests` in `syscall/api.rs` — is **not type-checked**
> in any routine build, so call sites there can silently drift (some still pass the old
> arg count to `Context::new`); do not treat that test module as a live gate, and don't
> churn it when threading new `Context` fields.

The two gates that actually protect the kernel are therefore:

1. **`cargo check -p neuron --target riscv64imac-unknown-none-elf`** (wrapped by
   `just diag-kernel`) — the real type/exhaustiveness/borrow gate. `neuron`'s
   `deny(warnings)` is active only on this target, so dead code and unused items are caught
   here, not on the host.
2. **QEMU boot + `KSELFTEST:` markers** — runtime behaviour, asserted by `scripts/qemu-test.sh`.

### When you still want a deterministic host oracle

Pure kernel *data-structure* logic (no MMIO, no router, no scheduler coupling) can and
should be host-unit-tested. The pattern, established by the RFC-0033 spine
(`source/kernel/neuron/src/waitset.rs`, `source/kernel/neuron/src/fence.rs`):

- Declare the module **un-gated** in `lib.rs` (the only modules without
  `#[cfg(target_os = "none")]`), with a comment stating why.
- Keep it free of riscv-only types: use `alloc` (available on host and target) and raw
  `u32`/`u64` ids instead of `EndpointId`/`Pid`/`Cap`. The gated syscall layer does the
  id ↔ kernel-type mapping; the table stays portable.
- Add `#[cfg_attr(not(target_os = "none"), allow(dead_code))]` at the module top: on the
  host the only consumer is the test module (the syscall layer that uses it is gated out),
  so this silences host dead-code noise while the riscv build keeps strict checks.
- Write the `#[cfg(test)] mod tests` as normal — they now run under `cargo test -p neuron`
  and are the deterministic oracle (QEMU timing never decides correctness).

The syscall *integration* (dispatch → table → router/scheduler) is not host-testable this
way; prove it with a `KSELFTEST:` marker added to `selftest/mod.rs` instead.

## CI pipeline

CI lives under `.github/workflows/`:

- `ci.yml`: host-first checks (fmt/clippy/tests, remote E2E, Miri, deadcode scan) and a bounded QEMU run via `scripts/qemu-test.sh`.
- `build.yml`: build verification (includes `make initial-setup` and `make build MODE=host`; optional OS smoke job).

On failure, CI uploads `uart.log` / `qemu.log` to aid triage. Determinism is enforced via stable marker strings and marker-driven early exit.
