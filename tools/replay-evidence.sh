#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Phase-6 replay entrypoint for evidence bundles.
# Replays a sealed bundle deterministically by:
#   1) verifying signature,
#   2) pinning git-SHA,
#   3) replaying `just test-os PROFILE=<recorded>` with recorded env,
#   4) diffing original vs replay trace via tools/diff-traces.sh.
#
# OWNERS: @runtime
# STATUS: Functional (TASK-0023B P6-01)

set -Eeuo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/replay-evidence.sh <bundle.tar.gz> --max-seconds=<N> [options]

Required:
  --max-seconds=<N>     hard replay budget in seconds (must be > 0)

Options:
  --git-sha=<sha>       override recorded build SHA (used by bisect tooling)
  --report-json=<path>  write machine-readable replay summary
  --log-file=<path>     append replay step logs + harness output
  --worktree-dir=<path> persistent detached worktree path (default: target/replay-worktree)
  --keep-workdir=1      keep temporary replay worktree for debugging
  -h, --help            show this help

Behavior:
  - verifies bundle signature via tools/verify-evidence.sh --policy=any
  - reads recorded profile/env/kernel_cmdline/build_sha from config.json
  - runs replay in a detached git worktree pinned to the selected SHA
  - executes: just test-os <recorded-profile>
  - compares trace.jsonl against the original bundle via tools/diff-traces.sh
  - fails closed if replay requires unsupported/extra environment
EOF
  exit 2
}

die() {
  echo "[replay][error] $*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

bundle=""
max_seconds=""
override_sha=""
report_json=""
log_file=""
worktree_dir=""
keep_workdir=0

for arg in "$@"; do
  case "$arg" in
    --max-seconds=*) max_seconds="${arg#--max-seconds=}" ;;
    --git-sha=*) override_sha="${arg#--git-sha=}" ;;
    --report-json=*) report_json="${arg#--report-json=}" ;;
    --log-file=*) log_file="${arg#--log-file=}" ;;
    --worktree-dir=*) worktree_dir="${arg#--worktree-dir=}" ;;
    --keep-workdir=1) keep_workdir=1 ;;
    -h|--help) usage ;;
    -*)
      die "unknown flag: $arg"
      ;;
    *)
      if [[ -z "$bundle" ]]; then
        bundle="$arg"
      else
        die "only one bundle path is accepted"
      fi
      ;;
  esac
done

[[ -n "$bundle" ]] || usage
[[ -f "$bundle" ]] || die "bundle not found: $bundle"
[[ -n "$max_seconds" ]] || die "--max-seconds is mandatory"
[[ "$max_seconds" =~ ^[0-9]+$ ]] || die "--max-seconds must be an integer"
(( max_seconds > 0 )) || die "--max-seconds must be > 0"

require_cmd git
require_cmd python3
require_cmd tar
require_cmd timeout
require_cmd just

ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel 2>/dev/null || true)
[[ -n "$ROOT" ]] || die "failed to resolve repository root"

VERIFY_SCRIPT="$ROOT/tools/verify-evidence.sh"
DIFF_SCRIPT="$ROOT/tools/diff-traces.sh"
[[ -x "$VERIFY_SCRIPT" ]] || die "missing executable verify script: $VERIFY_SCRIPT"
[[ -x "$DIFF_SCRIPT" ]] || die "missing executable diff script: $DIFF_SCRIPT"

resolve_nebin() {
  local cand
  if [[ -n "${NEXUS_EVIDENCE_BIN:-}" && -x "${NEXUS_EVIDENCE_BIN:-}" ]]; then
    printf '%s\n' "$NEXUS_EVIDENCE_BIN"
    return 0
  fi
  for cand in "$ROOT/target/debug/nexus-evidence" "$ROOT/target/release/nexus-evidence"; do
    if [[ -x "$cand" ]]; then
      printf '%s\n' "$cand"
      return 0
    fi
  done
  return 1
}

nebin=$(resolve_nebin || true)
[[ -n "$nebin" ]] || die "nexus-evidence binary not found; run: cargo build -p nexus-evidence --bin nexus-evidence"

resolve_pm_bin() {
  local cand
  if [[ -n "${NEXUS_PROOF_MANIFEST_BIN:-}" && -x "${NEXUS_PROOF_MANIFEST_BIN:-}" ]]; then
    printf '%s\n' "$NEXUS_PROOF_MANIFEST_BIN"
    return 0
  fi
  for cand in "$ROOT/target/debug/nexus-proof-manifest" "$ROOT/target/release/nexus-proof-manifest"; do
    if [[ -x "$cand" ]]; then
      printf '%s\n' "$cand"
      return 0
    fi
  done
  return 1
}

pm_bin=$(resolve_pm_bin || true)
[[ -n "$pm_bin" ]] || die "nexus-proof-manifest binary not found; run: cargo build -p nexus-proof-manifest --bin nexus-proof-manifest"

if [[ -z "$log_file" ]]; then
  mkdir -p "$ROOT/.cursor"
  log_file="$ROOT/.cursor/replay-evidence.log"
fi
if [[ "$log_file" != /* ]]; then
  log_file="$ROOT/$log_file"
fi
mkdir -p "$(dirname "$log_file")"
: >"$log_file"

if [[ -z "$worktree_dir" ]]; then
  worktree_dir="$ROOT/target/replay-worktree"
elif [[ "$worktree_dir" != /* ]]; then
  worktree_dir="$ROOT/$worktree_dir"
fi
mkdir -p "$(dirname "$worktree_dir")"

log() {
  local ts
  ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  echo "[$ts] $*" | tee -a "$log_file" >&2
}

start_epoch=$(date +%s)
deadline_epoch=$((start_epoch + max_seconds))
remaining_budget() {
  local now
  now=$(date +%s)
  local rem=$((deadline_epoch - now))
  if (( rem < 0 )); then
    rem=0
  fi
  printf '%s\n' "$rem"
}

replay_target_dir="${REPLAY_CARGO_TARGET_DIR:-$ROOT/target/replay-cache}"
mkdir -p "$replay_target_dir"
artifact_stamp="$replay_target_dir/.replay-last-sha"

log "[replay] start bundle=$bundle max_seconds=$max_seconds target_cache=$replay_target_dir"
log "[replay] using evidence_bin=$nebin"
log "[replay] using proof_manifest_bin=$pm_bin"
log "[replay] using worktree_dir=$worktree_dir"
if [[ -n "${NEXUS_EVIDENCE_CI_PUBKEY:-}" || -n "${NEXUS_EVIDENCE_BRINGUP_PUBKEY:-}" ]]; then
  log "[replay] note: overriding external NEXUS_EVIDENCE_*_PUBKEY env for deterministic verify"
fi

tmpdir=$(mktemp -d)
cleanup() {
  if [[ "$keep_workdir" == "1" ]]; then
    echo "[replay][info] keeping workdir: $tmpdir" >&2
    return
  fi
  rm -rf "$tmpdir"
}
trap cleanup EXIT

extract_dir="$tmpdir/extracted"
mkdir -p "$extract_dir"

orig_trace="$extract_dir/original-trace.jsonl"
orig_config="$extract_dir/original-config.json"

tar -xOf "$bundle" trace.jsonl >"$orig_trace" \
  || die "bundle missing trace.jsonl: $bundle"
tar -xOf "$bundle" config.json >"$orig_config" \
  || die "bundle missing config.json: $bundle"

log "[replay] verify signature: $bundle"
env \
  -u NEXUS_EVIDENCE_CI_PUBKEY \
  -u NEXUS_EVIDENCE_BRINGUP_PUBKEY \
  NEXUS_EVIDENCE_BIN="$nebin" \
  "$VERIFY_SCRIPT" "$bundle" --policy=any >/dev/null

config_vars="$tmpdir/config.vars"
python3 - "$orig_config" >"$config_vars" <<'PY'
import json
import sys

cfg_path = sys.argv[1]
cfg = json.load(open(cfg_path, "r", encoding="utf-8"))
profile = cfg.get("profile", "")
build_sha = cfg.get("build_sha", "")
kernel_cmdline = cfg.get("kernel_cmdline", "")
qemu_args = cfg.get("qemu_args", [])
env = cfg.get("env", {})

if not profile:
    print("ERR=missing profile")
    sys.exit(0)
if not build_sha:
    print("ERR=missing build_sha")
    sys.exit(0)
if not isinstance(qemu_args, list):
    print("ERR=qemu_args is not a list")
    sys.exit(0)
if qemu_args:
    print("ERR=non-empty qemu_args unsupported in replay skeleton")
    sys.exit(0)
if not isinstance(env, dict):
    print("ERR=env is not an object")
    sys.exit(0)

print(f"PROFILE={profile}")
print(f"BUILD_SHA={build_sha}")
print(f"KERNEL_CMDLINE={kernel_cmdline}")
for k in sorted(env):
    v = env[k]
    if not isinstance(v, str):
        print(f"ERR=env value for {k!r} is non-string")
        sys.exit(0)
    print("ENV\t{}\t{}".format(k, v))
PY

recorded_profile=""
recorded_sha=""
recorded_kernel_cmdline=""
declare -a replay_env_pairs=()

while IFS= read -r line; do
  case "$line" in
    ERR=*) die "${line#ERR=}" ;;
    PROFILE=*) recorded_profile="${line#PROFILE=}" ;;
    BUILD_SHA=*) recorded_sha="${line#BUILD_SHA=}" ;;
    KERNEL_CMDLINE=*) recorded_kernel_cmdline="${line#KERNEL_CMDLINE=}" ;;
    ENV$'\t'*)
      kv="${line#ENV$'\t'}"
      k="${kv%%$'\t'*}"
      v="${kv#*$'\t'}"
      [[ "$k" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || die "invalid env key in bundle: $k"
      replay_env_pairs+=("$k=$v")
      ;;
    "") ;;
    *)
      die "unexpected config parse line: $line"
      ;;
  esac
done <"$config_vars"

[[ -n "$recorded_profile" ]] || die "recorded profile missing"
[[ -n "$recorded_sha" ]] || die "recorded build_sha missing"

# Hard gate: replay must use only bundle-recorded run controls.
for blocked in \
  PROFILE SELFTEST_PROFILE RUN_PHASE REQUIRE_SMP REQUIRE_DSOFTBUS \
  REQUIRE_QEMU_DHCP REQUIRE_QEMU_DHCP_STRICT KERNEL_CMDLINE; do
  if [[ -n "${!blocked:-}" ]]; then
    die "environment override $blocked is set; replay requires bundle-recorded run controls only"
  fi
done

replay_sha="$recorded_sha"
if [[ -n "$override_sha" ]]; then
  replay_sha="$override_sha"
fi

git -C "$ROOT" cat-file -e "${replay_sha}^{commit}" \
  || die "git SHA does not exist: $replay_sha"

worktree="$worktree_dir"
if [[ ! -e "$worktree/.git" ]]; then
  log "[replay] creating detached worktree at $worktree sha=$replay_sha"
  git -C "$ROOT" worktree add --quiet --detach "$worktree" "$replay_sha"
else
  git -C "$worktree" rev-parse --is-inside-work-tree >/dev/null 2>&1 \
    || die "existing worktree_dir is not a valid git worktree: $worktree"
  log "[replay] reusing existing worktree at $worktree and switching to sha=$replay_sha"
  git -C "$worktree" checkout --detach --force "$replay_sha" >/dev/null
  git -C "$worktree" clean -fdq >/dev/null
fi

evidence_dir="$worktree/target/evidence"
mkdir -p "$evidence_dir"
before_list="$tmpdir/before-evidence.lst"
after_list="$tmpdir/after-evidence.lst"
ls -1 "$evidence_dir"/*.tar.gz 2>/dev/null | sort >"$before_list" || true

rem=$(remaining_budget)
(( rem > 0 )) || die "budget exhausted before replay run started"
log "[replay] run profile=$recorded_profile sha=$replay_sha remaining_budget=${rem}s"
replay_skip_build=0
if [[ -f "$artifact_stamp" ]]; then
  last_sha=$(tr -d ' \n\r' <"$artifact_stamp" || true)
  if [[ "$last_sha" == "$replay_sha" ]]; then
    replay_skip_build=1
  fi
fi
log "[replay] build_policy NEXUS_SKIP_BUILD=$replay_skip_build"
run_started=$(date +%s)
(
  cd "$worktree"
  set +e
  env \
    -u NEXUS_EVIDENCE_CI_PUBKEY \
    -u NEXUS_EVIDENCE_BRINGUP_PUBKEY \
    -u NEXUS_EVIDENCE_BRINGUP_PRIVKEY \
    -u NEXUS_EVIDENCE_BRINGUP_DIR \
    "${replay_env_pairs[@]}" \
    KERNEL_CMDLINE="$recorded_kernel_cmdline" \
    CARGO_TARGET_DIR="$replay_target_dir" \
    NEXUS_EVIDENCE_BIN="$nebin" \
    NEXUS_PROOF_MANIFEST_BIN="$pm_bin" \
    NEXUS_SKIP_BUILD="$replay_skip_build" \
    RUN_TIMEOUT="${rem}s" \
    timeout --foreground --signal=TERM --kill-after=20s "${rem}s" \
      just test-os "$recorded_profile" >>"$log_file" 2>&1
  rc=$?
  set -e
  if [[ "$rc" -eq 124 ]]; then
    die "replay command timed out after ${rem}s (see $log_file)"
  fi
  if [[ "$rc" -ne 0 ]]; then
    die "replay command failed rc=$rc (see $log_file)"
  fi
)
run_elapsed=$(( $(date +%s) - run_started ))
log "[replay] run_elapsed_seconds=$run_elapsed"
printf '%s\n' "$replay_sha" >"$artifact_stamp"

ls -1 "$evidence_dir"/*.tar.gz 2>/dev/null | sort >"$after_list" || true
new_bundle=$(comm -13 "$before_list" "$after_list" | tail -n1 || true)
if [[ -z "$new_bundle" ]]; then
  new_bundle=$(ls -1t "$evidence_dir"/*-"$recorded_profile"-*.tar.gz 2>/dev/null | head -n1 || true)
fi
[[ -n "$new_bundle" ]] || die "failed to locate replay-produced evidence bundle in $evidence_dir"
[[ -f "$new_bundle" ]] || die "replay bundle path is not a file: $new_bundle"

replay_trace="$tmpdir/replay-trace.jsonl"
replay_config="$tmpdir/replay-config.json"
tar -xOf "$new_bundle" trace.jsonl >"$replay_trace" \
  || die "replay bundle missing trace.jsonl: $new_bundle"
tar -xOf "$new_bundle" config.json >"$replay_config" \
  || die "replay bundle missing config.json: $new_bundle"

diff_json="$tmpdir/trace-diff.json"
if "$DIFF_SCRIPT" "$orig_trace" "$replay_trace" --format=json >"$diff_json"; then
  diff_exit=0
else
  diff_exit=$?
fi

# P6-05 enforcement mechanics:
# allow only host-surface drift fields in config; everything else must match.
config_cmp_json="$tmpdir/config-compare.json"
python3 - "$orig_config" "$replay_config" "$recorded_sha" "$replay_sha" >"$config_cmp_json" <<'PY'
import json
import sys
from copy import deepcopy

orig = json.load(open(sys.argv[1], "r", encoding="utf-8"))
repl = json.load(open(sys.argv[2], "r", encoding="utf-8"))
recorded_sha = sys.argv[3]
replay_sha = sys.argv[4]

allow_top = {"wall_clock_utc", "qemu_version", "host_info"}
allow_env = {"HOSTNAME"}
if replay_sha != recorded_sha:
    allow_top.add("build_sha")

def scrub(obj):
    o = deepcopy(obj)
    for k in allow_top:
        o.pop(k, None)
    env = o.get("env")
    if isinstance(env, dict):
        for k in list(env.keys()):
            if k in allow_env:
                env.pop(k, None)
    return o

o = scrub(orig)
r = scrub(repl)
ok = o == r
out = {
    "ok": ok,
    "allowlist_top_level": sorted(allow_top),
    "allowlist_env_keys": sorted(allow_env),
}
if not ok:
    out["original_scrubbed"] = o
    out["replay_scrubbed"] = r
print(json.dumps(out, sort_keys=True))
PY

config_ok=$(python3 - "$config_cmp_json" <<'PY'
import json,sys
obj=json.load(open(sys.argv[1], "r", encoding="utf-8"))
print("1" if obj.get("ok") else "0")
PY
)

summary_json="$tmpdir/replay-summary.json"
python3 - \
  "$bundle" "$new_bundle" "$orig_trace" "$replay_trace" "$diff_json" "$config_cmp_json" "$recorded_profile" "$replay_sha" \
  >"$summary_json" <<'PY'
import json
import sys

(
    original_bundle,
    replay_bundle,
    original_trace,
    replay_trace,
    diff_json,
    config_cmp_json,
    profile,
    replay_sha,
) = sys.argv[1:]

diff = json.load(open(diff_json, "r", encoding="utf-8"))
cfg = json.load(open(config_cmp_json, "r", encoding="utf-8"))
print(json.dumps({
    "original_bundle": original_bundle,
    "replay_bundle": replay_bundle,
    "original_trace": original_trace,
    "replay_trace": replay_trace,
    "profile": profile,
    "replay_sha": replay_sha,
    "trace_diff": diff,
    "config_compare": cfg,
}, sort_keys=True))
PY

if [[ -n "$report_json" ]]; then
  cp "$summary_json" "$report_json"
fi

if [[ "$diff_exit" -ne 0 ]]; then
  echo "[replay][error] trace replay mismatch (see diff output below)" >&2
  cat "$diff_json" >&2
  log "[replay] trace diff mismatch"
  exit "$diff_exit"
fi

if [[ "$config_ok" != "1" ]]; then
  echo "[replay][error] replay config drift exceeds allowlist" >&2
  cat "$config_cmp_json" >&2
  log "[replay] config allowlist mismatch"
  exit 1
fi

log "[replay] ok trace matched"
log "[replay] replay bundle: $new_bundle"
log "[replay] summary path: $summary_json"
log "[replay] report path: ${report_json:-<not-written>}"
log "[replay] finished"

