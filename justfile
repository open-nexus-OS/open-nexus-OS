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
    @echo "  just test-os             # run kernel selftests in QEMU"
    @echo "  just test-smp            # deterministic SMP dual-mode proof ladder (SMP=2 gate + SMP=1 parity)"
    @echo "  just test-mmio           # run QEMU until MMIO phase is complete"
    @echo "  just test-os-dhcp         # QEMU smoke with DHCP requested (bounded, deterministic fallback allowed)"
    @echo "  just test-os-dhcp-strict  # QEMU smoke with strict DHCP gate (requires net: dhcp bound)"
    @echo "  just test-dsoftbus-2vm    # TASK-0005: 2-VM DSoftBus harness"
    @echo "  just test-dsoftbus-2vm-pcap # 2-VM DSoftBus harness + PCAP capture"
    @echo "  just test-dsoftbus-mux    # TASK-0020: requirement-named mux host suites"
    @echo "  just test-dsoftbus-quic   # TASK-0021: host QUIC transport + selection suites"
    @echo "  just test-dsoftbus-host   # full userspace/dsoftbus host regression"
    @echo "  just qemu                # boot kernel in QEMU (manual)"
    @echo "  just test-init           # run host init test (nexus-init spawns daemons)"
    @echo "  INIT_LITE_LOG_TOPICS=svc-meta just qemu  # opt-in init-lite log topics"
    @echo "  tools/uart-filter.py --strip-escape uart.log | less  # decode legacy escape logs"
    @echo
    @echo "[Project Maintainers]"
    @echo "  just lint                # run clippy checks"
    @echo "  just fmt-check           # verify rustfmt formatting"
    @echo "  just arch-check          # userspace/kernel layering guard"
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
    RUN_TIMEOUT=${RUN_TIMEOUT:-90s} RUN_UNTIL_MARKER="${RUN_UNTIL_MARKER:-SELFTEST: dsoftbus ping ok}" scripts/run-qemu-rv64.sh {{args}}
    @echo "[hint] Default stop marker is 'SELFTEST: dsoftbus ping ok'. Set RUN_UNTIL_MARKER=1 for full readiness ladder or QEMU_TRACE=1 for tracing."

test-os:
    scripts/qemu-test.sh
    @echo "[hint] Kernel triage: illegal-instruction dumps sepc/scause/stval+bytes; enable trap_symbols for name+offset; post-SATP marker validates return path."

# Deterministic SMP ladder:
# - SMP=2 with REQUIRE_SMP=1 (enforces SMP-only marker contract)
# - SMP=1 parity run (preserve single-hart baseline behavior)
test-smp:
    SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-190s} just test-os
    SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-190s} just test-os

# QEMU smoke variants (networking / DSoftBus gates).
#
# IMPORTANT: run these sequentially (not in parallel) to avoid blk.img lock contention.
test-os-dhcp:
    # #region agent log
    python -c 'import json,time;open("/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log","a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H3","location":"justfile:test-os-dhcp:start","message":"start target","data":{"target":"test-os-dhcp"},"timestamp":int(time.time()*1000)})+"\n")'
    # #endregion
    bash -lc 'set -o pipefail; REQUIRE_QEMU_DHCP=1 RUN_TIMEOUT=${RUN_TIMEOUT:-190s} just test-os 2>&1 | tee "/home/jenning/open-nexus-OS/.cursor/test-os-dhcp.output.log"; rc=${PIPESTATUS[0]}; RC="$rc" python -c '\''import json,os,time,pathlib; p=pathlib.Path("/home/jenning/open-nexus-OS/.cursor/test-os-dhcp.output.log"); lines=p.read_text(encoding="utf-8",errors="ignore").splitlines() if p.exists() else []; warns=[ln.strip() for ln in lines if ("warning:" in ln.lower() or "[warn" in ln.lower())]; dbg="/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log"; open(dbg,"a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H6","location":"justfile:test-os-dhcp:warning-scan","message":"captured warning lines","data":{"target":"test-os-dhcp","warningCount":len(warns),"warningSample":warns[:8],"exitCode":int(os.environ["RC"])},"timestamp":int(time.time()*1000)})+"\n"); open(dbg,"a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H3","location":"justfile:test-os-dhcp:end","message":"target completed","data":{"target":"test-os-dhcp","exitCode":int(os.environ["RC"])},"timestamp":int(time.time()*1000)})+"\n")'\''; exit $rc'

test-os-dhcp-strict:
    # #region agent log
    python -c 'import json,time;open("/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log","a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H3","location":"justfile:test-os-dhcp-strict:start","message":"start target","data":{"target":"test-os-dhcp-strict"},"timestamp":int(time.time()*1000)})+"\n")'
    # #endregion
    bash -lc 'set -o pipefail; REQUIRE_QEMU_DHCP=1 REQUIRE_QEMU_DHCP_STRICT=1 RUN_TIMEOUT=${RUN_TIMEOUT:-190s} just test-os 2>&1 | tee "/home/jenning/open-nexus-OS/.cursor/test-os-dhcp-strict.output.log"; rc=${PIPESTATUS[0]}; RC="$rc" python -c '\''import json,os,time,pathlib; p=pathlib.Path("/home/jenning/open-nexus-OS/.cursor/test-os-dhcp-strict.output.log"); lines=p.read_text(encoding="utf-8",errors="ignore").splitlines() if p.exists() else []; warns=[ln.strip() for ln in lines if ("warning:" in ln.lower() or "[warn" in ln.lower())]; dbg="/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log"; open(dbg,"a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H7","location":"justfile:test-os-dhcp-strict:warning-scan","message":"captured warning lines","data":{"target":"test-os-dhcp-strict","warningCount":len(warns),"warningSample":warns[:8],"exitCode":int(os.environ["RC"])},"timestamp":int(time.time()*1000)})+"\n"); open(dbg,"a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H3","location":"justfile:test-os-dhcp-strict:end","message":"target completed","data":{"target":"test-os-dhcp-strict","exitCode":int(os.environ["RC"])},"timestamp":int(time.time()*1000)})+"\n")'\''; exit $rc'

# Run only until device-MMIO proofs are complete (faster local iteration).
test-mmio:
    RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=${RUN_TIMEOUT:-190s} just test-os

test-init:
    scripts/host-init-test.sh

# -----------------------------------------------------------------------------
# Opt-in OS 2-VM harness (TASK-0005)
# -----------------------------------------------------------------------------

os2vm:
    @RUN_OS2VM=1 RUN_TIMEOUT=${RUN_TIMEOUT:-180s} tools/os2vm.sh

os2vm-pcap:
    @RUN_OS2VM=1 OS2VM_PCAP=1 RUN_TIMEOUT=${RUN_TIMEOUT:-180s} tools/os2vm.sh

# Friendlier aliases for DSoftBus bring-up.
test-dsoftbus-2vm:
    # #region agent log
    python -c 'import json,time;open("/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log","a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H4","location":"justfile:test-dsoftbus-2vm:start","message":"start target","data":{"target":"test-dsoftbus-2vm"},"timestamp":int(time.time()*1000)})+"\n")'
    # #endregion
    bash -lc 'set -o pipefail; just os2vm 2>&1 | tee "/home/jenning/open-nexus-OS/.cursor/test-dsoftbus-2vm.output.log"; rc=${PIPESTATUS[0]}; RC="$rc" python -c '\''import json,os,time,pathlib; p=pathlib.Path("/home/jenning/open-nexus-OS/.cursor/test-dsoftbus-2vm.output.log"); lines=p.read_text(encoding="utf-8",errors="ignore").splitlines() if p.exists() else []; warns=[ln.strip() for ln in lines if ("warning:" in ln.lower() or "[warn" in ln.lower())]; dbg="/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log"; open(dbg,"a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H8","location":"justfile:test-dsoftbus-2vm:warning-scan","message":"captured warning lines","data":{"target":"test-dsoftbus-2vm","warningCount":len(warns),"warningSample":warns[:8],"exitCode":int(os.environ["RC"])},"timestamp":int(time.time()*1000)})+"\n"); open(dbg,"a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H4","location":"justfile:test-dsoftbus-2vm:end","message":"target completed","data":{"target":"test-dsoftbus-2vm","exitCode":int(os.environ["RC"])},"timestamp":int(time.time()*1000)})+"\n")'\''; exit $rc'

test-dsoftbus-2vm-pcap:
    just os2vm-pcap

# TASK-0020 requirement-named host suites (deterministic contract surface).
test-dsoftbus-mux:
    # #region agent log
    python -c 'import json,time;open("/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log","a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H2","location":"justfile:test-dsoftbus-mux:start","message":"start target","data":{"target":"test-dsoftbus-mux"},"timestamp":int(time.time()*1000)})+"\n")'
    # #endregion
    cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture
    cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture
    cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture
    # #region agent log
    python -c 'import json,time;open("/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log","a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H2","location":"justfile:test-dsoftbus-mux:end","message":"target completed","data":{"target":"test-dsoftbus-mux"},"timestamp":int(time.time()*1000)})+"\n")'
    # #endregion

# TASK-0021 targeted host QUIC proof suites (real transport + selection/reject contract).
test-dsoftbus-quic:
    cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture
    cargo test -p dsoftbus --test quic_selection_contract -- --nocapture

# Full userspace dsoftbus host regression (includes mux + reject suites).
test-dsoftbus-host:
    # #region agent log
    python -c 'import json,time;open("/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log","a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H2","location":"justfile:test-dsoftbus-host:start","message":"start target","data":{"target":"test-dsoftbus-host"},"timestamp":int(time.time()*1000)})+"\n")'
    # #endregion
    cargo test -p dsoftbus -- --nocapture
    # #region agent log
    python -c 'import json,time;open("/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log","a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H2","location":"justfile:test-dsoftbus-host:end","message":"target completed","data":{"target":"test-dsoftbus-host"},"timestamp":int(time.time()*1000)})+"\n")'
    # #endregion

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
    @env RUSTFLAGS='{{host_rustflags}}' cargo test -p nexus-e2e -p remote_e2e -p logd-e2e -p vfs-e2e -p e2e_policy

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
    # #region agent log
    python -c 'import json,time;open("/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log","a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H5","location":"justfile:test-all:start","message":"start aggregate gate","data":{"target":"test-all"},"timestamp":int(time.time()*1000)})+"\n")'
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
    just test-smp
    # #region agent log
    python -c 'import json,time;open("/home/jenning/open-nexus-OS/.cursor/debug-98eb36.log","a",encoding="utf-8").write(json.dumps({"sessionId":"98eb36","runId":"pre-fix","hypothesisId":"H5","location":"justfile:test-all:end","message":"aggregate gate completed","data":{"target":"test-all"},"timestamp":int(time.time()*1000)})+"\n")'
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
    @env RUSTFLAGS='{{os_rustflags}} -W unexpected_cfgs -W dead_code' cargo +{{toolchain}} check -p netstackd -p dsoftbusd -p keystored -p policyd -p samgrd -p bundlemgrd -p packagefsd -p vfsd -p execd -p timed -p metricsd --target riscv64imac-unknown-none-elf --no-default-features --features os-lite --message-format=short

# Kernel-only: quickest way to see unused/dead_code in neuron.
diag-kernel:
    cargo +{{toolchain}} check -p neuron --target riscv64imac-unknown-none-elf --message-format=short

# -----------------------------------------------------------------------------
# License & Advisory Check (cargo-deny)
# -----------------------------------------------------------------------------

deny-check:
    @echo "==> cargo-deny check (licenses + advisories)"
    @cargo deny check --config config/deny.toml

# -----------------------------------------------------------------------------
# Dependency Hygiene Gate (RFC-0009)
# -----------------------------------------------------------------------------

# Forbidden crates that MUST NOT appear in the OS/QEMU dependency graph.
# See docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md for rationale.
forbidden_crates := "parking_lot parking_lot_core getrandom"

# Check OS dependency graph for forbidden crates (RFC-0009 Phase 2 enforcement).
# Fails with exit code 1 if any forbidden crate is found.
dep-gate:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "==> RFC-0009 Dependency Hygiene Gate"
    echo "    Forbidden crates: {{forbidden_crates}}"
    echo "    Target: riscv64imac-unknown-none-elf (OS/QEMU slice)"
    echo ""
    # OS services to check (must match justfile diag-os and Makefile)
    services="dsoftbusd netstackd keystored policyd samgrd bundlemgrd packagefsd vfsd execd timed metricsd"
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
