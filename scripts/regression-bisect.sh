#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: CI-friendly wrapper for evidence replay bisect (P6-04).
# Typical flow:
#   1) take last-green bundle as "good"
#   2) take failed-run bundle as "bad"
#   3) run bounded bisect to locate first regressing commit
#
# OWNERS: @runtime
# STATUS: Functional (TASK-0023B P6-04)

set -Eeuo pipefail

usage() {
  cat >&2 <<'EOF'
usage: scripts/regression-bisect.sh --good-bundle=<path> --bad-bundle=<path> \
  --max-commits=<N> --max-seconds=<N> [--per-replay-seconds=<N>] [--worktree-dir=<path>] [--report-json=<path>]

Environment fallbacks (if flags omitted):
  REGRESSION_GOOD_BUNDLE
  REGRESSION_BAD_BUNDLE
  REGRESSION_MAX_COMMITS
  REGRESSION_MAX_SECONDS
  REGRESSION_PER_REPLAY_SECONDS
  REGRESSION_WORKTREE_DIR

Exit code mirrors tools/bisect-evidence.sh.
EOF
  exit 2
}

good_bundle="${REGRESSION_GOOD_BUNDLE:-}"
bad_bundle="${REGRESSION_BAD_BUNDLE:-}"
max_commits="${REGRESSION_MAX_COMMITS:-}"
max_seconds="${REGRESSION_MAX_SECONDS:-}"
per_replay="${REGRESSION_PER_REPLAY_SECONDS:-}"
worktree_dir="${REGRESSION_WORKTREE_DIR:-}"
log_file="${REGRESSION_LOG_FILE:-}"
report_json=""

for arg in "$@"; do
  case "$arg" in
    --good-bundle=*) good_bundle="${arg#--good-bundle=}" ;;
    --bad-bundle=*) bad_bundle="${arg#--bad-bundle=}" ;;
    --max-commits=*) max_commits="${arg#--max-commits=}" ;;
    --max-seconds=*) max_seconds="${arg#--max-seconds=}" ;;
    --per-replay-seconds=*) per_replay="${arg#--per-replay-seconds=}" ;;
    --worktree-dir=*) worktree_dir="${arg#--worktree-dir=}" ;;
    --log-file=*) log_file="${arg#--log-file=}" ;;
    --report-json=*) report_json="${arg#--report-json=}" ;;
    -h|--help) usage ;;
    *)
      echo "[regression-bisect][error] unknown arg: $arg" >&2
      usage
      ;;
  esac
done

[[ -n "$good_bundle" && -n "$bad_bundle" && -n "$max_commits" && -n "$max_seconds" ]] || usage

ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "$0")/.." && pwd))
BISECT_SCRIPT="$ROOT/tools/bisect-evidence.sh"
[[ -x "$BISECT_SCRIPT" ]] || {
  echo "[regression-bisect][error] missing executable: $BISECT_SCRIPT" >&2
  exit 1
}

cmd=(
  "$BISECT_SCRIPT"
  "$good_bundle"
  "$bad_bundle"
  "--max-commits=$max_commits"
  "--max-seconds=$max_seconds"
)
if [[ -n "$per_replay" ]]; then
  cmd+=("--per-replay-seconds=$per_replay")
fi
if [[ -n "$worktree_dir" ]]; then
  cmd+=("--worktree-dir=$worktree_dir")
fi
if [[ -n "$log_file" ]]; then
  cmd+=("--log-file=$log_file")
fi
if [[ -n "$report_json" ]]; then
  cmd+=("--report-json=$report_json")
fi

echo "[regression-bisect][info] good=$good_bundle" >&2
echo "[regression-bisect][info] bad=$bad_bundle" >&2
echo "[regression-bisect][info] max_commits=$max_commits max_seconds=$max_seconds per_replay=${per_replay:-default}" >&2
echo "[regression-bisect][info] worktree_dir=${worktree_dir:-default}" >&2
echo "[regression-bisect][info] log_file=${log_file:-default}" >&2

exec "${cmd[@]}"

