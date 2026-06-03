#!/usr/bin/env bash
# Copyright 2025 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Host-first fmt/clippy/cargo-deny checks for git hooks and CI
# OWNERS: @tools-team
# STATUS: Functional
# API_STABILITY: Stable
# TEST_COVERAGE: No tests
# ADR: docs/architecture/02-selftest-and-ci.md
#
# Logs to build/logs/fmt-clippy-deny-<timestamp>/ for post-run diagnostics.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_TARGET_DIR="${NEXUS_CARGO_TARGET_DIR:-${REPO_ROOT}/target}"

TIMESTAMP=$(date +%Y-%m-%dT%H-%M-%S)
LOG_DIR="${REPO_ROOT}/build/logs/fmt-clippy-deny-${TIMESTAMP}"
mkdir -p "$LOG_DIR"

echo "[info] Logging to $LOG_DIR" >&2

# ── Format check ──────────────────────────────────────────────
echo "[check] rustfmt --check" | tee "$LOG_DIR/fmt.log"
if ! cargo fmt --all -- --config-path config/rustfmt.toml --check >"$LOG_DIR/fmt.log" 2>&1; then
  echo "error: rustfmt check failed. See $LOG_DIR/fmt.log" >&2
  echo "error: Fix with: cargo +stable fmt --all -- --config-path config/rustfmt.toml" >&2
  echo "error: Kernel: cargo +nightly-2025-01-15 fmt -p neuron -p neuron-boot -- --config-path config/rustfmt.toml" >&2
  exit 1
fi
echo "[pass] rustfmt" >&2

# ── Clippy (host workspace) ───────────────────────────────────
RUSTFLAGS_DEFAULT='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"'
RUSTFLAGS="${RUSTFLAGS:-$RUSTFLAGS_DEFAULT}"

echo "[check] cargo clippy --workspace" | tee "$LOG_DIR/clippy.log"
if ! RUSTFLAGS="${RUSTFLAGS}" cargo clippy \
  --workspace \
  --all-targets \
  --exclude neuron \
  --exclude neuron-boot \
  -- -D warnings >"$LOG_DIR/clippy.log" 2>&1; then
  echo "error: clippy failed. See $LOG_DIR/clippy.log" >&2
  exit 1
fi
echo "[pass] clippy" >&2

# ── Kernel clippy (optional) ──────────────────────────────────
if [ "${KERNEL_LINT:-0}" = "1" ]; then
  NIGHTLY="${NIGHTLY:-nightly-2025-01-15}"
  echo "[check] cargo clippy -p neuron (kernel)" | tee "$LOG_DIR/clippy-kernel.log"
  if ! cargo +"${NIGHTLY}" clippy \
    -Z build-std=core,alloc -Z build-std-features=panic_immediate_abort \
    --target riscv64imac-unknown-none-elf -p neuron -- -D warnings >"$LOG_DIR/clippy-kernel.log" 2>&1; then
    echo "error: kernel clippy failed. See $LOG_DIR/clippy-kernel.log" >&2
    exit 1
  fi
  echo "[pass] clippy (kernel)" >&2
fi

# ── cargo-deny ────────────────────────────────────────────────
echo "[check] cargo-deny" | tee "$LOG_DIR/deny.log"
if command -v cargo-deny >/dev/null 2>&1; then
  if cargo deny check --config config/deny.toml >"$LOG_DIR/deny.log" 2>&1; then
    echo "[pass] cargo-deny" >&2
  else
    echo "warn: cargo-deny check failed (tooling/advisory-db issue?). See $LOG_DIR/deny.log" >&2
    if [ "${CI:-}" = "true" ] || [ "${DENY_STRICT:-0}" = "1" ]; then
      exit 1
    fi
  fi
else
  echo "warn: cargo-deny not found; skipping" >&2
  if [ "${CI:-}" = "true" ] || [ "${DENY_STRICT:-0}" = "1" ]; then
    exit 1
  fi
fi

echo "[pass] fmt-clippy-deny: all checks passed" >&2
echo "Logs: $LOG_DIR/" >&2
