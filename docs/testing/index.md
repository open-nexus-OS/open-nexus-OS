<!--
Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
-->

# Testing Methodology & Workflow

The Open Nexus testing strategy is **host-first, OS-last**. We emphasize fast,
deterministic feedback on the developer workstation, reserving QEMU for
end-to-end smoke coverage. This document walks through the testing layers,
required tooling, and day-to-day workflow.

## Philosophy

- **Host-first:** exercise as much logic as possible in userspace crates with
  unit, property, and contract tests.
- **OS-last:** reserve kernel and QEMU runs for ABI, IPC, and integration
  validation.
- **Determinism:** run the same commands locally and in CI via Podman images to
  avoid “works on my machine” surprises.

## Testing Layers

### Kernel (`kernel/`)

- Built with `#![no_std]`; only core logic and platform code live here.
- Self-tests print distinctive UART markers (`SELFTEST: end`, `samgrd: ready`,
  `bundlemgrd: ready`) so automation can detect success without parsing gigabyte
  logs.
- Golden vectors guard ABI and wire formats. Pure data structures can be split
  into host shims for property testing.
- Miri is not applicable except for isolated, host-only utility crates extracted
  from the kernel.

### Userspace Libraries (`userspace/`)

- Compile with `#![forbid(unsafe_code)]` and must stay Miri-friendly.
- Prefer `proptest`/property tests alongside unit and contract tests.
- Generate and validate golden vectors for IDL definitions in tandem with
  tooling in `tools/`.
- Error models are explicit—avoid `unwrap`/`expect` and exhaustively handle
  failure cases.

### Services & Daemons (`source/services/*d`)

- Thin IPC adapters that translate protocol messages to userspace library calls.
- Exercise IDL round-trips with local runners; there should be no `unwrap`/`expect`.
- Keep business logic in userspace crates so daemons remain testable shims.

## Workflow Checklist

1. **Extend userspace tests:** update the relevant crate and run
   `cargo test --workspace`.
2. **Run Miri where applicable:** `cargo miri test -p <crate>` for host-only
   libraries.
3. **Refresh golden vectors:** regenerate ABI/wire artifacts and bump semver if
   breaking changes occur.
4. **Touching the kernel?** Add or update self-tests that emit new UART markers
   and keep host logic split out when possible.
5. **Update the Podman dev container:**
   ```bash
   podman build -t open-nexus-os-dev -f podman/Devfile
   ```
   Re-enter the container to confirm matching toolchains/targets with CI.
6. **Full workspace tests:** run `cargo test --workspace` both locally and inside
   the container for parity.
7. **OS smoke/E2E:**
   - `just test-os` → bounded QEMU run that asserts UART markers.
   - `just qemu` → manual QEMU session; respects `RUN_TIMEOUT` and log trimming.

## Environment Parity & Prerequisites

- Toolchain pinned through `rust-toolchain.toml` (install matching channel).
- Targets: `rustup target add riscv64imac-unknown-none-elf`.
- System packages: `qemu-system-misc`, `capnproto`, `clang`, and friends (see
  container manifests).
- Use the Podman images in `podman/` to mirror CI dependencies—avoid relying on
  host-only tooling.

## House Rules

- No `unwrap`/`expect` in services/daemons.
- Userspace crates must compile with `#![forbid(unsafe_code)]` and clean clippy
  output.
- Kernel changes require UART markers for self-tests and must keep IDL parsing
  out of the runtime path.
- CI enforces architecture guards, UART marker presence, and linting baselines.

## Troubleshooting & Tips

- Increase or decrease the QEMU timeout with `RUN_TIMEOUT=60s just qemu` (or the
  Makefile equivalent). Timeouts prevent hung CI jobs.
- Adjust log caps by exporting `QEMU_LOG_MAX` or `UART_LOG_MAX`; trimming keeps
  artifacts under control (historically logs ballooned to ~50 GiB without caps).
- Enable fast-fail runs with `RUN_UNTIL_MARKER=1 just qemu` to stop as soon as a
  success marker is printed.
- Logs live in `qemu.log` and `uart.log` after each run; they are already
  trimmed to the configured limits.
- When debugging locally, disable marker mode with `RUN_UNTIL_MARKER=0` to keep
  QEMU running interactively.
