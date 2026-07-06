# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set shell := ["/usr/bin/env", "bash", "-c"]

toolchain := "nightly-2025-01-15"
cargo_target_dir := env_var_or_default("NEXUS_CARGO_TARGET_DIR", justfile_directory() / "target")

# Common flags (suppress unexpected_cfgs and set nexus_env)
host_rustflags := "--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"host\""
os_rustflags   := "--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\""

export CARGO_TARGET_DIR := cargo_target_dir

default: help

# -----------------------------------------------------------------------------
# DSL toolchain (TASK-0075): builds the nx-dsl backend and runs it directly,
# or through the `nx dsl` shim (NX_DSL_BACKEND delegation).
#   just dsl lint ui/pages/Home.nx      just dsl build -o target/dsl app.nx
# -----------------------------------------------------------------------------
dsl *ARGS:
    @cargo build -q -p nx-dsl
    @target/debug/nx-dsl {{ARGS}}

# The same via the `nx` CLI shim (proves the delegation contract).
nx-dsl-shim ACTION *ARGS:
    @cargo build -q -p nx-dsl -p nx
    @NX_DSL_BACKEND=target/debug/nx-dsl target/debug/nx dsl {{ACTION}} -- {{ARGS}}

# -----------------------------------------------------------------------------
# Help & Task Catalog
# -----------------------------------------------------------------------------
help:
    @echo "Open Nexus OS - common tasks:\n"
    @echo "[Developers: Host]"
    @echo "  just test-host           # run host test suite (exclude kernel)"
    @echo "  just test-e2e            # run host E2E tests (nexus-e2e + remote_e2e + logd-e2e + vfs-e2e + e2e_policy)"
    @echo "  just fmt-check           # check formatting (stable + kernel on nightly)"
    @echo "  just lint                # clippy (host cfg, exclude kernel)"
    @echo "  just miri-strict         # miri (no FS/network) for samgr,bundlemgr"
    @echo "  just miri-fs             # miri with FS isolation disabled"
    @echo
    @echo "[Kernel Developers]"
    @echo "  just build-kernel        # cross-compile kernel (riscv)"
    @echo "  just build-nexus-log-os  # cross-compile nexus-log (userspace sink)"
    @echo "  just build-init-lite-os  # cross-compile init-lite userspace payload"
    @echo "  just test-os             # run kernel selftests in QEMU (default profile=headless)"
    @echo "  just ci-os-headless       # headless CI (no display, full service chain)"
    @echo "  just ci-os-display-gpu    # GPU pipeline verification via UART markers"
    @echo "  just test-os visible-bootstrap # full visible UI test (requires GTK display)"
    @echo "  just test-os smp         # SMP-gated QEMU smoke (REQUIRE_SMP enforced via manifest)"
    @echo "  just ci-os-smp           # canonical SMP CI recipe (SMP=2 gate + SMP=1 parity)"
    @echo "  just test-mmio           # run QEMU until MMIO phase is complete"
    @echo "  just ci-os-dhcp           # QEMU smoke with DHCP requested (deterministic fallback allowed)"
    @echo "  just ci-os-dhcp-strict    # QEMU smoke with strict DHCP gate (requires net: dhcp bound)"
    @echo "  just ci-os-os2vm          # 2-VM DSoftBus QEMU harness via PROFILE=os2vm"
    @echo "  just test-dsoftbus-2vm-pcap # 2-VM DSoftBus harness + PCAP capture (legacy; keep for diagnostics)"
    @echo "  just test-dsoftbus-mux    # TASK-0020: requirement-named mux host suites"
    @echo "  just test-dsoftbus-quic   # TASK-0021: host QUIC transport + selection suites"
    @echo "  just test-dsoftbus-host   # full userspace/dsoftbus host regression"
    @echo "  just ci-network           # PROFILE-driven aggregate (replaces legacy test-network)"
    @echo "  just qemu                # boot kernel in QEMU (manual)"
    @echo "  just start               # build + launch interactive OS with full breadcrumbs"
    @echo "  just test-init           # run host init test (nexus-init spawns daemons)"
    @echo "  INIT_LITE_LOG_TOPICS=svc-meta just qemu  # opt-in init-lite log topics"
    @echo "  tools/uart-filter.py --strip-escape uart.log | less  # decode legacy escape logs"
    @echo
    @echo "[Project Maintainers]"
    @echo "  just lint                # run clippy checks"
    @echo "  just fmt-check           # verify rustfmt formatting"
    @echo "  just arch-check          # userspace/kernel layering guard"
    @echo "  just arch-gate           # selftest-client structural gate (ADR-0027)"
    @echo "  just deny-check          # cargo-deny license/advisory check"
    @echo "  just test-all            # host tests + miri + arch-check + kernel selftests"
    @echo
    @echo "[Diagnostics (match rust-analyzer / editor)]"
    @echo "  just diag-host           # cargo check (host cfg) with check-cfg + warnings"
    @echo "  just diag-os             # cargo check (os cfg, riscv target) with check-cfg + warnings"
    @echo "  just diag-kernel         # cargo check neuron (riscv target) with warnings"
    @echo "  just dep-gate            # RFC-0009: check OS graph for forbidden crates"
    @echo "  just os2vm               # TASK-0005: opt-in 2-VM QEMU harness (cross-VM DSoftBus)"
    @echo "  just os2vm-pcap          # same as os2vm but captures PCAPs for Wireshark"
    @echo
    @echo "[Build Cache]"
    @echo "  CARGO_TARGET_DIR defaults to <repo>/target for just recipes"
    @echo "  NEXUS_CARGO_TARGET_DIR=/path/to/target just test-all  # override target dir"

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
    RUN_TIMEOUT=${RUN_TIMEOUT:-90s} RUN_UNTIL_MARKER="${RUN_UNTIL_MARKER:-1}" scripts/qemu-launcher.sh {{args}}
    @echo "[hint] Default stop mode is RUN_UNTIL_MARKER=1 (init+grace). Set RUN_TIMEOUT=0 for endless interactive run or QEMU_TRACE=1 for tracing."

start *args:
    # self-contained interactive path: build first, then keep the same guest
    # alive with the richer breadcrumb ladder. Default to the host build path so
    # interactive starts do not rebuild the dev container or compile cargo-udeps.
    #
    # UART verdict grid (see docs/adr/0040 §8): an interactive boot folds each subsystem/service's
    # markers into one `[ts] OK <group> N/N <ms>` line (compact; failures + slow groups stand out).
    # To see one group's FULL raw markers while the rest stay folded — the focus-while-debugging
    # lever — set NEXUS_LOG_EXPAND to a comma list of group names, e.g.:
    #     NEXUS_LOG_EXPAND=netstackd just start   # netstackd raw, everything else folded
    #     NEXUS_LOG_EXPAND=gpud just start        # gpud raw, netstackd folded
    # Nothing is hidden — folding is just the default view; expand recalls the raw stream per group.
    # (Proof boots `just test-os` never fold, so verify-uart sees every raw marker.)
    GPU_MODE=${GPU_MODE:-virgl} make MODE=${NEXUS_START_BUILD_MODE:-host} build
    NEXUS_SKIP_BUILD=1 NEXUS_DISPLAY_BOOTSTRAP=1 GPU_MODE=${GPU_MODE:-virgl} QEMU_SESSION_MODE=interactive QEMU_MARKER_LEVEL=full NEXUS_SELFTEST_MODE=interactive-full QEMU_PROOF_POINTER_SOURCE=${QEMU_PROOF_POINTER_SOURCE:-tablet} QEMU_DISPLAY_BACKEND=${QEMU_DISPLAY_BACKEND:-gtk} QEMU_GPU_XRES=${QEMU_GPU_XRES:-1280} QEMU_GPU_YRES=${QEMU_GPU_YRES:-800} RUN_UNTIL_MARKER=0 RUN_TIMEOUT=${RUN_TIMEOUT:-0} scripts/qemu-launcher.sh {{args}}
    @echo "[hint] just start defaults to GPU_MODE=virgl (real GL-scanout compositor) in a visible gtk,gl=on window. Use GPU_MODE=mmio just start for the CPU-fallback 2D path. If the GL window backgrounds go black it is the gpud GL-scanout path (gl_scanout.rs full-frame gl_present_damage / task #69), NOT a host/display bug — do not switch backends to 'fix' it."

# Interactive REAL GPU compositor (virgl) over an egl-headless + VNC pipe. This is
# the off-screen counterpart to `just start` (which now defaults to a visible
# virgl gtk,gl=on window): same GL-scanout compositor, served over VNC instead of
# a local window — handy for remote viewing or capture. VNC forwards mouse/keyboard
# to the guest. Needs a VNC viewer (none ships by default):
#   pacman -S tigervnc   → vncviewer localhost:5979
#   or krdc              → krdc vnc://localhost:5979
start-vnc *args:
    GPU_MODE=virgl make MODE=${NEXUS_START_BUILD_MODE:-host} build
    @echo "[VNC] real GPU compositor up — connect now:  vncviewer localhost:5979   (or  krdc vnc://localhost:5979)"
    NEXUS_SKIP_BUILD=1 NEXUS_DISPLAY_BOOTSTRAP=1 GPU_MODE=virgl QEMU_SESSION_MODE=interactive QEMU_MARKER_LEVEL=full NEXUS_SELFTEST_MODE=interactive-full QEMU_PROOF_POINTER_SOURCE=${QEMU_PROOF_POINTER_SOURCE:-tablet} QEMU_DISPLAY_BACKEND=egl-headless QEMU_EXTRA_ARGS="-vnc 127.0.0.1:79" QEMU_GPU_XRES=${QEMU_GPU_XRES:-1280} QEMU_GPU_YRES=${QEMU_GPU_YRES:-800} RUN_UNTIL_MARKER=0 RUN_TIMEOUT=${RUN_TIMEOUT:-0} scripts/qemu-launcher.sh {{args}}

# TASK-0023B P4-06: `test-os` now accepts an optional PROFILE arg that
# `scripts/qemu-test.sh` forwards to the manifest CLI (`nexus-proof-manifest
# list-env --profile=…`). Default `headless` runs without display.
# Use `just test-os full` for display (requires GTK), `just test-os headless` for CI.
# Migration target: invoke `just test-os <headless|smp|dhcp|quic-required|os2vm>`
# (positional arg; just 1.47 parses `PROFILE=foo` after the recipe name as
# another recipe name, not a parameter override) everywhere.
test-os profile='headless':
    scripts/qemu-test.sh --profile={{profile}}
    @echo "[hint] Kernel triage: illegal-instruction dumps sepc/scause/stval+bytes; enable trap_symbols for name+offset; post-SATP marker validates return path."

# Deterministic SMP ladder retired in TASK-0023B P4-10:
# `test-smp`/`test-os-dhcp`/`test-os-dhcp-strict` were soft-deprecated in
# P4-06 and have been deleted. The canonical recipes are now PROFILE-driven:
#   - SMP gate         : `just ci-os-smp`         (SMP=2 strict + SMP=1 parity)
#   - DHCP soft        : `just ci-os-dhcp`        (PROFILE=dhcp)
#   - DHCP strict      : `just ci-os-dhcp-strict` (PROFILE=dhcp-strict)
#   - 2-VM DSoftBus    : `just ci-os-os2vm`       (PROFILE=os2vm)
#   - QUIC required    : `just ci-os-quic`        (PROFILE=quic-required)
#   - Aggregate matrix : `just ci-network`        (replaces legacy test-network)
# All `REQUIRE_*` env knobs are sourced from `proof-manifest.toml` via
# `nexus-proof-manifest list-env --profile=<name>`; arch-gate Rule 6 prevents
# any new `REQUIRE_*` literal from leaking back into a `test-*` recipe.

# Run only until device-MMIO proofs are complete (faster local iteration).
test-mmio:
    RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-190s} just test-os

test-init:
    scripts/host-init-test.sh

# -----------------------------------------------------------------------------
# TASK-0023B P4-06 — CI matrix: profile-driven QEMU smoke recipes.
#
# These are the canonical `ci-*` entry points; all CI plumbing should call
# `just ci-<flavor>` rather than the soft-deprecated `test-os-dhcp` /
# `test-dsoftbus-2vm` / `test-network` family. Each recipe forwards a
# manifest profile to `qemu-test.sh`, which sources its env via
# `nexus-proof-manifest list-env --profile=<flavor>` (single source of truth).
# -----------------------------------------------------------------------------
ci-os-full:
    just test-os full

ci-os-headless:
    just contract-windowd-size
    just test-os headless

# CI contract: windowd image-size budget (spawn-time VMO-pool allocation).
# Silent-death prevention — see scripts/check-windowd-size.sh.
contract-windowd-size:
    @scripts/check-windowd-size.sh

ci-os-display-gpu-pci:
    GPU_MODE=pci just test-os display-gpu

# full visible UI test with PCI GPU (requires GTK display)
test-os-visible-pci:
    GPU_MODE=pci just test-os visible-bootstrap

ci-os-smp:
    SMP=2 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-300s} just test-os smp

ci-os-dhcp:
    RUN_TIMEOUT=${RUN_TIMEOUT:-190s} just test-os dhcp

# TASK-0023B P4-10: strict DHCP gate via PROFILE=dhcp-strict (manifest extends
# `dhcp` with REQUIRE_QEMU_DHCP_STRICT=1). Replaces the legacy
# `test-os-dhcp-strict` recipe deleted in P4-10.
ci-os-dhcp-strict:
    RUN_TIMEOUT=${RUN_TIMEOUT:-190s} just test-os dhcp-strict

ci-os-quic:
    RUN_TIMEOUT=${RUN_TIMEOUT:-190s} just test-os quic-required

ci-os-os2vm:
    RUN_OS2VM=1 RUN_TIMEOUT=${RUN_TIMEOUT:-180s} just test-os os2vm

# Aggregate: profile-driven network matrix (replacement for `test-network`).
ci-network:
    just ci-os-dhcp
    just ci-os-quic
    just ci-os-os2vm

# -----------------------------------------------------------------------------
# TASK-0023B P4-08 — runtime-profile QEMU smoke recipes.
#
# These recipes set `NEXUS_SELFTEST_PROFILE=<name>` at runtime through QEMU
# `fw_cfg`, so the same built artifacts can be reused across profile
# selections. The QEMU runner stays on the harness-side `full` profile
# (full env wiring); the OS binary itself decides which phases to actually
# execute, emitting `dbg: phase X skipped` for the others. Manifest /
# dispatcher contract is locked by `cargo test -p nexus-proof-manifest
# --test runtime_profiles`. Per-profile UART verification (deny-by-default
# on unexpected markers) lands in P4-09.
# -----------------------------------------------------------------------------
ci-os-runtime-bringup:
    NEXUS_SELFTEST_PROFILE=bringup just test-os headless

ci-os-runtime-quick:
    NEXUS_SELFTEST_PROFILE=quick just test-os headless

ci-os-runtime-none:
    NEXUS_SELFTEST_PROFILE=none just test-os headless

# -----------------------------------------------------------------------------
# Opt-in OS 2-VM harness (TASK-0005)
# -----------------------------------------------------------------------------

os2vm:
    @RUN_OS2VM=1 RUN_TIMEOUT=${RUN_TIMEOUT:-180s} tools/os2vm.sh

os2vm-pcap:
    @RUN_OS2VM=1 OS2VM_PCAP=1 RUN_TIMEOUT=${RUN_TIMEOUT:-180s} tools/os2vm.sh

# `test-dsoftbus-2vm` removed in TASK-0023B P4-10; use `just ci-os-os2vm`.
# The PCAP variant is preserved for diagnostic captures (Wireshark workflows).
test-dsoftbus-2vm-pcap:
    just os2vm-pcap

# TASK-0020 requirement-named host suites (deterministic contract surface).
test-dsoftbus-mux:
    # #region agent log
    python -c 'import json,time,os; p=os.environ.get("HYPOTHESIS_LOG"); sid=None; rec={"runId":"pre-fix","hypothesisId":"H2","location":"justfile:test-dsoftbus-mux:start","message":"start target","data":{"target":"test-dsoftbus-mux"},"timestamp":int(time.time()*1000)};  open(p,"a",encoding="utf-8").write(json.dumps(rec)+"\n") if p else None'
    # #endregion
    cargo +stable test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture
    cargo +stable test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture
    cargo +stable test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture
    # #region agent log
    python -c 'import json,time,os; p=os.environ.get("HYPOTHESIS_LOG"); sid=None; rec={"runId":"pre-fix","hypothesisId":"H2","location":"justfile:test-dsoftbus-mux:end","message":"target completed","data":{"target":"test-dsoftbus-mux"},"timestamp":int(time.time()*1000)};  open(p,"a",encoding="utf-8").write(json.dumps(rec)+"\n") if p else None'
    # #endregion

# TASK-0021 targeted host QUIC proof suites (real transport + selection/reject contract).
test-dsoftbus-quic:
    cargo +stable test -p dsoftbus --test quic_host_transport_contract -- --nocapture
    cargo +stable test -p dsoftbus --test quic_selection_contract -- --nocapture

# Full userspace dsoftbus host regression (includes mux + reject suites).
test-dsoftbus-host:
    # #region agent log
    python -c 'import json,time,os; p=os.environ.get("HYPOTHESIS_LOG"); sid=None; rec={"runId":"pre-fix","hypothesisId":"H2","location":"justfile:test-dsoftbus-host:start","message":"start target","data":{"target":"test-dsoftbus-host"},"timestamp":int(time.time()*1000)};  open(p,"a",encoding="utf-8").write(json.dumps(rec)+"\n") if p else None'
    # #endregion
    cargo +stable test -p dsoftbus -- --nocapture
    # #region agent log
    python -c 'import json,time,os; p=os.environ.get("HYPOTHESIS_LOG"); sid=None; rec={"runId":"pre-fix","hypothesisId":"H2","location":"justfile:test-dsoftbus-host:end","message":"target completed","data":{"target":"test-dsoftbus-host"},"timestamp":int(time.time()*1000)};  open(p,"a",encoding="utf-8").write(json.dumps(rec)+"\n") if p else None'
    # #endregion

# `test-network` removed in TASK-0023B P4-10; use `just ci-network` for the
# PROFILE-driven aggregate matrix (dhcp + quic-required + os2vm).

# -----------------------------------------------------------------------------
# Host test suites
# -----------------------------------------------------------------------------

fmt-check:
    @echo "==> rustfmt (stable)"
    @if ! cargo +stable fmt --all -- --config-path config/rustfmt.toml --check; then \
        echo "error: rustfmt check failed."; \
        echo "error: do not use plain 'cargo fmt'; use the repo-approved format commands:"; \
        echo "  cargo +stable fmt --all -- --config-path config/rustfmt.toml"; \
        echo "  cargo +nightly-2025-01-15 fmt -p neuron -p neuron-boot -- --config-path config/rustfmt.toml"; \
        exit 1; \
    fi
    @echo "==> rustfmt (kernel, nightly)"
    @rustup component add --toolchain {{toolchain}} rustfmt >/dev/null 2>&1 || true
    @if ! cargo +{{toolchain}} fmt -p neuron -p neuron-boot -- --config-path config/rustfmt.toml --check; then \
        echo "error: kernel rustfmt check failed."; \
        echo "error: use the repo-approved kernel format command:"; \
        echo "  cargo +nightly-2025-01-15 fmt -p neuron -p neuron-boot -- --config-path config/rustfmt.toml"; \
        exit 1; \
    fi

lint:
    @echo "==> clippy (host cfg, exclude kernel)"
    @env RUSTFLAGS='--cfg nexus_env="host"' cargo +stable clippy --workspace --exclude neuron --exclude neuron-boot -- -D warnings -D clippy::unwrap_used -D clippy::expect_used -W dead_code -A unexpected_cfgs

test-host:
    @echo "==> Running host test suite (exclude kernel)"
    @env RUSTFLAGS='{{host_rustflags}}' cargo +stable test --workspace --exclude neuron --exclude neuron-boot

# Pack the app bundles (`bundles/<app>/manifest.toml` → `target/bundles/<app>.nxb`).
# RFC-0065: chat/search ship as real `.nxb` bundles with Cap'n Proto manifests;
# bundlemgrd enumerates them and abilitymgr resolves the launch ability.
# NOTE: payload is the demo-exit0 ELF placeholder until each app crate ships its own
# ELF (TASK-0065 P4); the manifest is the canonical, signable artifact.
pack-bundles:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "==> Packing app bundles (RFC-0065)"
    out="target/bundles"
    mkdir -p "$out"
    placeholder="$out/.placeholder.elf"
    printf '\x7fELF nexus app payload placeholder' > "$placeholder"
    for app in chat search; do
        echo "--- $app ---"
        cargo run -q -p nxb-pack -- --toml "bundles/$app/manifest.toml" "$placeholder" "$out/$app.nxb"
        echo "    -> $out/$app.nxb/manifest.nxb"
    done
    echo "[ok] bundles packed under $out/"

test-e2e:
    @echo "==> Running host E2E tests"
    @env RUSTFLAGS='{{host_rustflags}}' cargo +stable test -p nexus-e2e -p remote_e2e -p logd-e2e -p vfs-e2e -p e2e_policy

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
    cargo +stable run -p arch-check

# -----------------------------------------------------------------------------
# Aggregates
# -----------------------------------------------------------------------------

test-all:
    # #region agent log
    python -c 'import json,time,os; p=os.environ.get("HYPOTHESIS_LOG"); sid=None; rec={"runId":"pre-fix","hypothesisId":"H5","location":"justfile:test-all:start","message":"start aggregate gate","data":{"target":"test-all"},"timestamp":int(time.time()*1000)};  open(p,"a",encoding="utf-8").write(json.dumps(rec)+"\n") if p else None'
    # #endregion
    just fmt-check
    just lint
    just deny-check
    just test-host
    just test-e2e
    just miri-strict
    just miri-fs
    just arch-check
    just build-kernel
    just ci-os-smp
    # #region agent log
    python -c 'import json,time,os; p=os.environ.get("HYPOTHESIS_LOG"); sid=None; rec={"runId":"pre-fix","hypothesisId":"H5","location":"justfile:test-all:end","message":"aggregate gate completed","data":{"target":"test-all"},"timestamp":int(time.time()*1000)};  open(p,"a",encoding="utf-8").write(json.dumps(rec)+"\n") if p else None'
    # #endregion

# -----------------------------------------------------------------------------
# Diagnostics (reproduce editor/rust-analyzer output)
# -----------------------------------------------------------------------------

# Host: enable cfg validation and surface warnings (including unexpected cfg).
# This intentionally excludes the kernel crates (they require nightly features).
diag-host:
    @echo "==> diag-host (toolchain=stable, nexus_env=host)"
    @rustc +stable -V
    @cargo +stable -V
    @env RUSTFLAGS='{{host_rustflags}} -W unexpected_cfgs -W dead_code' cargo +stable check --workspace --exclude neuron --exclude neuron-boot --all-targets --message-format=short

# OS: enable cfg validation and surface warnings for riscv builds (os-lite style).
# Note: OS builds are a *slice* (kernel + init-lite + OS services). Do not use --all-targets on bare-metal
# as it pulls in cfg(test) paths and host-only crates which is not representative.
diag-os:
    @echo "==> diag-os (toolchain={{toolchain}}, target=riscv64imac-unknown-none-elf, nexus_env=os)"
    @rustc +{{toolchain}} -V
    @cargo +{{toolchain}} -V
    @echo "==> kernel libs (neuron)"
    @cargo +{{toolchain}} check -p neuron --target riscv64imac-unknown-none-elf --message-format=short
    @echo "==> userspace payload (init-lite)"
    @env RUSTFLAGS='{{os_rustflags}} -W unexpected_cfgs -W dead_code' cargo +{{toolchain}} check -p init-lite --target riscv64imac-unknown-none-elf --message-format=short
    @echo "==> OS services (os-lite feature set)"
    @env RUSTFLAGS='{{os_rustflags}} -W unexpected_cfgs -W dead_code' cargo +{{toolchain}} check -p netstackd -p dsoftbusd -p keystored -p policyd -p samgrd -p bundlemgrd -p packagefsd -p vfsd -p execd -p abilitymgr -p timed -p metricsd --target riscv64imac-unknown-none-elf --no-default-features --features os-lite --message-format=short

# Kernel-only: quickest way to see unused/dead_code in neuron.
diag-kernel:
    cargo +{{toolchain}} check -p neuron --target riscv64imac-unknown-none-elf --message-format=short

# -----------------------------------------------------------------------------
# License & Advisory Check (cargo-deny)
# -----------------------------------------------------------------------------

deny-check:
    @echo "==> cargo-deny check (licenses + advisories)"
    @cargo +stable deny check --config config/deny.toml

# -----------------------------------------------------------------------------
# Architecture Gate (TASK-0023B P3-03 / RFC-0038 refinement (7) / ADR-0027)
# -----------------------------------------------------------------------------

# Mechanical structural enforcement for selftest-client (5 rules; allowlists in
# source/apps/selftest-client/.arch-allowlist.txt). Cheap; runs before dep-gate
# so structural drift fails fast.
arch-gate:
    bash scripts/check-selftest-arch.sh

# -----------------------------------------------------------------------------
# Dependency Hygiene Gate (RFC-0009)
# -----------------------------------------------------------------------------

# Forbidden crates that MUST NOT appear in the OS/QEMU dependency graph.
# See docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md for rationale.
forbidden_crates := "parking_lot parking_lot_core getrandom"

# Check OS dependency graph for forbidden crates (RFC-0009 Phase 2 enforcement).
# Fails with exit code 1 if any forbidden crate is found.
# Chains arch-gate first so the cheap structural check fails fast (TASK-0023B P3-03).
dep-gate: arch-gate
    #!/usr/bin/env bash
    set -euo pipefail
    echo "==> RFC-0009 Dependency Hygiene Gate"
    echo "    Forbidden crates: {{forbidden_crates}}"
    echo "    Target: riscv64imac-unknown-none-elf (OS/QEMU slice)"
    echo ""
    # OS services to check (must match justfile diag-os and Makefile)
    services="dsoftbusd netstackd keystored policyd samgrd bundlemgrd packagefsd vfsd execd abilitymgr timed metricsd"
    found_forbidden=0
    for svc in $services; do
        echo "--- Checking $svc ---"
        # Get dependency tree for this service with os-lite features
        tree_output=$(cargo +{{toolchain}} tree -p "$svc" --target riscv64imac-unknown-none-elf --no-default-features --features os-lite 2>&1 || true)
        for forbidden in {{forbidden_crates}}; do
            if echo "$tree_output" | grep -qE "^[│├└ ]*$forbidden "; then
                echo "[FAIL] Found forbidden crate '$forbidden' in $svc dependency graph!"
                echo "$tree_output" | grep -E "$forbidden" | head -5
                found_forbidden=1
            fi
        done
    done
    echo ""
    if [[ "$found_forbidden" -eq 1 ]]; then
        echo "[FAIL] RFC-0009 dependency hygiene violated!"
        echo "       Fix: Use --no-default-features --features os-lite for all OS crates."
        echo "       See: docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md"
        exit 1
    else
        echo "[PASS] RFC-0009 dependency hygiene: no forbidden crates in OS graph."
    fi
