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

## CI pipeline

CI lives under `.github/workflows/`:

- `ci.yml`: host-first checks (fmt/clippy/tests, remote E2E, Miri, deadcode scan) and a bounded QEMU run via `scripts/qemu-test.sh`.
- `build.yml`: build verification (includes `make initial-setup` and `make build MODE=host`; optional OS smoke job).

On failure, CI uploads `uart.log` / `qemu.log` to aid triage. Determinism is enforced via stable marker strings and marker-driven early exit.
