# Testing methodology

Open Nexus OS follows a **host-first, OS-last** strategy. Most logic is exercised with fast host tools, leaving QEMU for end-to-end smoke coverage only. This document explains the layers, expectations, and day-to-day workflow for contributors.

## Philosophy
- Prioritise fast feedback by writing unit, property, and contract tests in userspace crates first.
- Keep kernel selftests focused on syscall, IPC, and VMO surface validation; they should advertise success through UART markers that CI can detect.
- Reserve QEMU for smoke and integration validation. Runs are bounded by a timeout and produce trimmed logs to avoid multi-gigabyte artefacts.

## Testing layers
### Kernel (`source/kernel/neuron`)
- `#![no_std]` runtime with selftests that emit UART markers such as `SELFTEST: begin` and `SELFTEST: end`.
- Exercise hardware-adjacent code paths (traps, scheduler, IPC router). Golden vectors capture ABI and wire format expectations.
- Use host shims for pure data structures that can be unit-tested outside QEMU; Miri is not applicable except for such extracted shims.

### Userspace libraries (`userspace/`)
- All crates compile with `#![forbid(unsafe_code)]` and are structured to run on the host toolchain.
- Favour `cargo test --workspace`, `proptest`, and `cargo miri test` (e.g. `cargo miri test -p samgr`).
- Golden vectors (IDL definitions, ABI structures) live here and drive service contract expectations.

### Services and daemons (`source/services/*d`)
- Daemons are thin IPC adapters that translate requests into calls to userspace libraries. They must avoid `unwrap`/`expect` in favour of rich error types.
- Provide IDL round-trip and contract tests using the local runner tools. Keep business logic in the userspace crates so daemons stay lean.

## Workflow checklist
1. Expand or add tests in the relevant userspace library. Run `cargo test --workspace` until green.
2. For eligible crates (pure Rust, host compatible), run Miri: `cargo miri test -p <crate>`.
3. Update or record Golden Vectors when wire formats or IDL definitions change. Bump SemVer if the change is breaking.
4. Touching the kernel? Update or add selftests so they print distinct UART markers, and keep complicated logic in host-side shims where possible.
5. Rebuild the Podman development container to ensure parity with CI:
   - `podman build -t open-nexus-os-dev -f podman/Containerfile`
   - Enter the container and confirm the toolchain/targets match CI.
6. Execute full workspace tests both locally and inside the container: `cargo test --workspace`.
7. Finish with OS-level smoke/E2E coverage: `just test-os` (uses QEMU with timeouts and UART assertions). For manual boot loops, run `just qemu`.

## Environment parity & prerequisites
- Toolchain pinned via `rust-toolchain.toml`; install the listed version before building.
- Targets required: `rustup target add riscv64imac-unknown-none-elf`.
- System dependencies: `qemu-system-misc`, `capnproto`, and supporting build packages. The Podman container image installs the same dependencies for CI parity.
- Do not rely on host-only tools—update `recipes/` or container definitions when new packages are needed.

## House rules
- No `unwrap`/`expect` in daemons; propagate errors with context.
- Userspace crates must keep `#![forbid(unsafe_code)]` enabled and pass Clippy’s denied lints.
- CI enforces architecture guards, UART markers, and formatting; keep commits green locally before pushing.

## Troubleshooting tips
- QEMU runs are bounded by the `RUN_TIMEOUT` environment variable (default `30s`). Increase it only when debugging: `RUN_TIMEOUT=120s just qemu`.
- Logs are trimmed post-run. Override caps with `QEMU_LOG_MAX` or `UART_LOG_MAX` if you need to preserve more context.
- Enable marker-driven early exit for faster loops by setting `RUN_UNTIL_MARKER=1` (already defaulted in `just test-os`). Logs appear as `qemu.log` (diagnostics) and `uart.log` (console output) in the working directory.
- For stubborn host/container mismatches, rebuild the Podman image and ensure the same targets are installed inside and outside the container.
