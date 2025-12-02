# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set shell := ["/usr/bin/env", "bash", "-c"]

toolchain := "nightly-2025-01-15"

# Common flags (suppress unexpected_cfgs and set nexus_env)
host_rustflags := "--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\""
os_rustflags   := "--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\""

default: help

# -----------------------------------------------------------------------------
# Help & Task Catalog
# -----------------------------------------------------------------------------
help:
    @echo "Open Nexus OS - common tasks:\n"
    @echo "[Developers: Host]"
    @echo "  just test-host           # run host test suite (exclude kernel)"
    @echo "  just test-e2e            # run host E2E tests (nexus-e2e + remote_e2e)"
    @echo "  just fmt-check           # check formatting (stable + kernel on nightly)"
    @echo "  just lint                # clippy (host cfg, exclude kernel)"
    @echo "  just miri-strict         # miri (no FS/network) for samgr,bundlemgr"
    @echo "  just miri-fs             # miri with FS isolation disabled"
    @echo
    @echo "[Kernel Developers]"
    @echo "  just build-kernel        # cross-compile kernel (riscv)"
    @echo "  just build-nexus-log-os  # cross-compile nexus-log (userspace sink)"
    @echo "  just build-init-lite-os  # cross-compile init-lite userspace payload"
    @echo "  just test-os             # run kernel selftests in QEMU"
    @echo "  just qemu                # boot kernel in QEMU (manual)"
    @echo "  just test-init           # run host init test (nexus-init spawns daemons)"
    @echo "  INIT_LITE_LOG_TOPICS=svc-meta just qemu  # opt-in init-lite log topics"
    @echo
    @echo "[Project Maintainers]"
    @echo "  just lint                # run clippy checks"
    @echo "  just fmt-check           # verify rustfmt formatting"
    @echo "  just arch-check          # userspace/kernel layering guard"
    @echo "  just test-all            # host tests + miri + arch-check + kernel selftests"

# Build the bootable NEURON binary crate
build-kernel:
    cargo +{{toolchain}} build -p neuron-boot --target riscv64imac-unknown-none-elf --release

# Cross-compile the shared logging shim with the OS sink
build-nexus-log-os:
    @env RUSTFLAGS='{{os_rustflags}}' cargo +{{toolchain}} build -p nexus-log --features sink-userspace --target riscv64imac-unknown-none-elf --release

# Cross-compile the os-lite init payload
build-init-lite-os:
    @env RUSTFLAGS='{{os_rustflags}}' cargo +{{toolchain}} build -p init-lite --target riscv64imac-unknown-none-elf --release

# Build only the kernel library with its own panic handler (no binary)
build-kernel-lib:
    cargo +{{toolchain}} build -p neuron --lib --features panic_handler --target riscv64imac-unknown-none-elf


qemu *args:
    # ensure the binary is built before launching
    just build-kernel
    RUN_TIMEOUT=${RUN_TIMEOUT:-30s} scripts/run-qemu-rv64.sh {{args}}
    @echo "[hint] Set RUN_UNTIL_MARKER=1 to stop on success markers; set QEMU_TRACE=1 to enable tracing."

test-os:
    scripts/qemu-test.sh
    @echo "[hint] Kernel triage: illegal-instruction dumps sepc/scause/stval+bytes; enable trap_symbols for name+offset; post-SATP marker validates return path."

test-init:
    scripts/host-init-test.sh

# -----------------------------------------------------------------------------
# Host test suites
# -----------------------------------------------------------------------------

fmt-check:
    @echo "==> rustfmt (stable)"
    @cargo +stable fmt --all -- --config-path config/rustfmt.toml --check
    @echo "==> rustfmt (kernel, nightly)"
    @rustup component add --toolchain {{toolchain}} rustfmt >/dev/null 2>&1 || true
    @cargo +{{toolchain}} fmt -p neuron -p neuron-boot -- --config-path config/rustfmt.toml --check

lint:
    @echo "==> clippy (host cfg, exclude kernel)"
    @env RUSTFLAGS='--cfg nexus_env="host"' cargo +stable clippy --workspace --exclude neuron --exclude neuron-boot -- -D warnings -D clippy::unwrap_used -D clippy::expect_used -W dead_code -A unexpected_cfgs

test-host:
    @echo "==> Running host test suite (exclude kernel)"
    @env RUSTFLAGS='{{host_rustflags}}' cargo test --workspace --exclude neuron --exclude neuron-boot

test-e2e:
    @echo "==> Running host E2E tests"
    @env RUSTFLAGS='{{host_rustflags}}' cargo test -p nexus-e2e -p remote_e2e

# Back-compat alias
test:
    just test-host

# -----------------------------------------------------------------------------
# Miri (memory model)
# -----------------------------------------------------------------------------

miri-strict:
    @RUSTUP_TOOLCHAIN={{toolchain}} cargo miri setup
    @env MIRIFLAGS='--cfg nexus_env="host"' RUSTUP_TOOLCHAIN={{toolchain}} cargo miri test -p identity

miri-fs:
    @RUSTUP_TOOLCHAIN={{toolchain}} cargo miri setup
    @env MIRIFLAGS='-Zmiri-disable-isolation --cfg nexus_env="host"' RUSTUP_TOOLCHAIN={{toolchain}} cargo miri test -p samgr -p bundlemgr

arch-check:
    cargo run -p arch-check

# -----------------------------------------------------------------------------
# Aggregates
# -----------------------------------------------------------------------------

test-all:
    just test-host
    just test-e2e
    just miri-strict
    just miri-fs
    just arch-check
    just build-kernel
    just test-os
