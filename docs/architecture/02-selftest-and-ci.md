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

Marker details drift quickly; keep them centralized in the harness and the testing guide:

- Marker contract: `scripts/qemu-test.sh`
- Testing guide (methodology + marker sequence notes): `docs/testing/index.md`

## CI pipeline

CI lives under `.github/workflows/`:

- `ci.yml`: host-first checks (fmt/clippy/tests, remote E2E, Miri, deadcode scan) and a bounded QEMU run via `scripts/qemu-test.sh`.
- `build.yml`: build verification (includes `make initial-setup` and `make build MODE=host`; optional OS smoke job).

On failure, CI uploads `uart.log` / `qemu.log` to aid triage. Determinism is enforced via stable marker strings and marker-driven early exit.
