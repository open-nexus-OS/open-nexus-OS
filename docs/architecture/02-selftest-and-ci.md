<!-- Copyright 2024 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# NEURON Selftest Harness and Continuous Integration

NEURON v0 introduces an in-kernel selftest harness that executes immediately
after the deterministic boot sequence. The kernel emits a fixed set of UART
markers to make automated validation straightforward:

```
NEURON
boot: ok
traps: ok
sys: ok
SELFTEST: begin
SELFTEST: time ok
SELFTEST: ipc ok
SELFTEST: caps ok
SELFTEST: map ok
SELFTEST: sched ok
SELFTEST: end
```

The selftest task lives inside the kernel binary and exercises the bootstrap
scheduler, capability table, IPC router, and Sv39 mapping logic. Dedicated
assertion macros print failures with a `SELFTEST: fail` prefix so panic
handling can report the fault and dump register state. Kernel boot and
selftests consume deterministic time sources: the scheduler slice and timer
wakeup values are derived from `determinism::fixed_tick_ns()` and any future
randomised decisions must come from `determinism::seed()`.

## QEMU runner

`scripts/qemu-test.sh` launches the kernel under QEMU in headless mode,
records UART output to `uart.log`, and asserts that every marker listed above
is present. The runner also enables tracing (`-d int,mmu,unimp`) and stores
QEMU diagnostics in `qemu.log`. Setting `DEBUG_QEMU=1` adds a GDB stub so
kernel failures can be debugged interactively.

## CI pipeline

`.github/workflows/ci.yml` wires these checks into the public CI pipeline. The
workflow performs the following steps on an Ubuntu runner:

1. Install the stable Rust toolchain plus the `riscv64imac-unknown-none-elf`
   target.
2. Install QEMU (`qemu-system-misc`).
3. Run `cargo fmt --all --check` and `cargo clippy --all-targets --all-features -D warnings`.
4. Execute `cargo nextest run --workspace` for fast unit and property tests.
5. Run Miri on host-first crates via `cargo miri test -p userspace-samgr -p userspace-bundlemgr`.
6. Launch `just test-os`, which delegates to the QEMU harness described above.

On failure the CI workflow uploads `uart.log` and `qemu.log` artifacts to aid
triage. Deterministic boot guarantees that repeated CI runs remain stable even
when QEMU timing fluctuates.
