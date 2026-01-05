#!/usr/bin/env bash
# Copyright 2025 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Host-first fmt/clippy/cargo-deny checks for git hooks and CI (non-destructive; no mass rewrites).
# OWNERS: @tools-team
# STATUS: Functional
# API_STABILITY: Stable
# TEST_COVERAGE: No tests
# ADR: docs/architecture/02-selftest-and-ci.md

set -euo pipefail

# Format check only (do not rewrite files in hooks/CI).
cargo fmt --all -- --config-path config/rustfmt.toml --check

# Host-first lint; avoid OS-only feature sets and no_std kernel (needs -Z build-std).
RUSTFLAGS_DEFAULT='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"'
RUSTFLAGS="${RUSTFLAGS:-$RUSTFLAGS_DEFAULT}"

RUSTFLAGS="${RUSTFLAGS}" cargo clippy \
  --workspace \
  --all-targets \
  --exclude neuron \
  --exclude neuron-boot \
  -- -D warnings

# Optional: lint kernel separately (only if explicitly desired)
#   KERNEL_LINT=1 NIGHTLY=nightly-2025-01-15 ./scripts/fmt-clippy-deny.sh
if [ "${KERNEL_LINT:-0}" = "1" ]; then
  NIGHTLY="${NIGHTLY:-nightly-2025-01-15}"
  cargo +"${NIGHTLY}" clippy \
    -Z build-std=core,alloc -Z build-std-features=panic_immediate_abort \
    --target riscv64imac-unknown-none-elf -p neuron -- -D warnings
fi

if command -v cargo-deny >/dev/null 2>&1; then
  if cargo deny check --config config/deny.toml; then
    :
  else
    echo "warn: cargo-deny check failed (tooling/advisory-db issue?)." >&2
    echo "      Hint: upgrade cargo-deny and/or refresh advisory DB (e.g. remove ~/.cargo/advisory-dbs/)." >&2
    # In CI or when explicitly requested, fail hard.
    if [ "${CI:-}" = "true" ] || [ "${DENY_STRICT:-0}" = "1" ]; then
      exit 1
    fi
  fi
else
  echo "warn: cargo-deny not found; skipping" >&2
  # In CI or when explicitly requested, fail hard (security gate must be present).
  if [ "${CI:-}" = "true" ] || [ "${DENY_STRICT:-0}" = "1" ]; then
    exit 1
  fi
fi
