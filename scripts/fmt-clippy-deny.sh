#!/usr/bin/env bash
# Copyright 2025 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Pre-commit gate — thin wrapper over the canonical just recipes
# OWNERS: @tools-team
# STATUS: Functional
# API_STABILITY: Stable
# TEST_COVERAGE: No tests
# ADR: docs/architecture/02-selftest-and-ci.md
#
# Delegates to `just fmt-check`, `just lint`, `just deny-check` so this hook can
# never drift from the canonical gate (it used to run its own, stricter clippy
# flag set and failed where `just lint`/CI passed). Set KERNEL_LINT=1 to add
# `just lint-kernel`. Logs to build/logs/fmt-clippy-deny-<timestamp>/.
#
# Wire as a git hook with:
#   ln -sf ../../scripts/fmt-clippy-deny.sh .git/hooks/pre-commit

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
export CARGO_TARGET_DIR="${NEXUS_CARGO_TARGET_DIR:-${REPO_ROOT}/target}"

TIMESTAMP=$(date +%Y-%m-%dT%H-%M-%S)
LOG_DIR="${REPO_ROOT}/build/logs/fmt-clippy-deny-${TIMESTAMP}"
mkdir -p "$LOG_DIR"
echo "[info] Logging to $LOG_DIR" >&2

run_gate() {
  local name=$1 recipe=$2
  echo "[check] just ${recipe}" >&2
  if ! just "$recipe" >"$LOG_DIR/${name}.log" 2>&1; then
    echo "error: just ${recipe} failed. See $LOG_DIR/${name}.log" >&2
    tail -n 20 "$LOG_DIR/${name}.log" >&2
    exit 1
  fi
  echo "[pass] ${recipe}" >&2
}

run_gate fmt fmt-check
run_gate clippy lint
if [ "${KERNEL_LINT:-0}" = "1" ]; then
  run_gate clippy-kernel lint-kernel
fi
run_gate deny deny-check

echo "[pass] fmt-clippy-deny: all checks passed" >&2
echo "Logs: $LOG_DIR/" >&2
