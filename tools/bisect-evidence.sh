#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Budgeted replay-bisect for evidence bundles (P6-03).
# Finds the first commit after <good> whose replay diverges from the
# good bundle's trace contract.
#
# OWNERS: @runtime
# STATUS: Functional (TASK-0023B P6-03)

set -Eeuo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/bisect-evidence.sh <good-bundle.tar.gz> <bad-bundle.tar.gz> \
  --max-commits=<N> --max-seconds=<N> [options]

Required:
  --max-commits=<N>         hard commit-budget (must be > 0)
  --max-seconds=<N>         hard wallclock budget for the full bisect (must be > 0)

Options:
  --per-replay-seconds=<N>  per-replay timeout budget (default: 300)
  --report-json=<path>      write machine-readable bisect summary
  --log-file=<path>         append bisect progress and replay log pointers
  --worktree-dir=<path>     persistent replay worktree for all bisect probes
  --synthetic-map=<path>    run algorithm-only smoke mode (no QEMU);
                            JSON: { "commits": [ { "sha": "...",
                            "verdict": "good|drift|bad" }, ... ] }
                            "drift" is treated as good for first-bad
                            detection but is reported separately so the
                            allowlist absorption can be audited.
  -h, --help                show this help

Exit codes:
  0  bisect completed and first bad commit identified
  1  no first bad found / budget exceeded / replay failed before classification
  2  usage error
EOF
  exit 2
}

die() {
  echo "[bisect][error] $*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

good_bundle=""
bad_bundle=""
max_commits=""
max_seconds=""
per_replay_seconds=300
report_json=""
log_file=""
worktree_dir=""
synthetic_map=""

for arg in "$@"; do
  case "$arg" in
    --max-commits=*) max_commits="${arg#--max-commits=}" ;;
    --max-seconds=*) max_seconds="${arg#--max-seconds=}" ;;
    --per-replay-seconds=*) per_replay_seconds="${arg#--per-replay-seconds=}" ;;
    --report-json=*) report_json="${arg#--report-json=}" ;;
    --log-file=*) log_file="${arg#--log-file=}" ;;
    --worktree-dir=*) worktree_dir="${arg#--worktree-dir=}" ;;
    --synthetic-map=*) synthetic_map="${arg#--synthetic-map=}" ;;
    -h|--help) usage ;;
    -*)
      die "unknown flag: $arg"
      ;;
    *)
      if [[ -z "$good_bundle" ]]; then
        good_bundle="$arg"
      elif [[ -z "$bad_bundle" ]]; then
        bad_bundle="$arg"
      else
        die "only two positional bundle paths are accepted"
      fi
      ;;
  esac
done

[[ -n "$good_bundle" && -n "$bad_bundle" ]] || usage
[[ -f "$good_bundle" ]] || die "good bundle not found: $good_bundle"
[[ -f "$bad_bundle" ]] || die "bad bundle not found: $bad_bundle"
[[ -n "$max_commits" ]] || die "--max-commits is mandatory"
[[ -n "$max_seconds" ]] || die "--max-seconds is mandatory"
[[ "$max_commits" =~ ^[0-9]+$ ]] || die "--max-commits must be an integer"
[[ "$max_seconds" =~ ^[0-9]+$ ]] || die "--max-seconds must be an integer"
[[ "$per_replay_seconds" =~ ^[0-9]+$ ]] || die "--per-replay-seconds must be an integer"
(( max_commits > 0 )) || die "--max-commits must be > 0"
(( max_seconds > 0 )) || die "--max-seconds must be > 0"
(( per_replay_seconds > 0 )) || die "--per-replay-seconds must be > 0"

require_cmd git
require_cmd python3

ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel 2>/dev/null || true)
[[ -n "$ROOT" ]] || die "failed to resolve repository root"

REPLAY_SCRIPT="$ROOT/tools/replay-evidence.sh"
VERIFY_SCRIPT="$ROOT/tools/verify-evidence.sh"
[[ -x "$REPLAY_SCRIPT" ]] || die "missing executable replay script: $REPLAY_SCRIPT"
[[ -x "$VERIFY_SCRIPT" ]] || die "missing executable verify script: $VERIFY_SCRIPT"

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

if [[ -z "$log_file" ]]; then
  mkdir -p "$ROOT/.cursor"
  log_file="$ROOT/.cursor/bisect-evidence.log"
fi
if [[ "$log_file" != /* ]]; then
  log_file="$ROOT/$log_file"
fi
mkdir -p "$(dirname "$log_file")"
: >"$log_file"

if [[ -z "$worktree_dir" ]]; then
  worktree_dir="$ROOT/target/replay-bisect-worktree"
elif [[ "$worktree_dir" != /* ]]; then
  worktree_dir="$ROOT/$worktree_dir"
fi
mkdir -p "$(dirname "$worktree_dir")"

log() {
  local ts
  ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  echo "[$ts] $*" | tee -a "$log_file" >&2
}

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

if [[ -n "$synthetic_map" ]]; then
  [[ -f "$synthetic_map" ]] || die "synthetic map not found: $synthetic_map"
  python3 - "$synthetic_map" "$max_commits" "$max_seconds" "$report_json" <<'PY'
import json
import sys
import time

path, max_commits, max_seconds, report_json = sys.argv[1:]
max_commits = int(max_commits)
max_seconds = int(max_seconds)
start = time.time()

doc = json.load(open(path, "r", encoding="utf-8"))
commits = doc.get("commits", [])
if not isinstance(commits, list) or not commits:
    print("[bisect][error] synthetic map has no commits", file=sys.stderr)
    raise SystemExit(1)
if len(commits) > max_commits:
    print(
        f"[bisect][error] synthetic commit count {len(commits)} exceeds --max-commits={max_commits}",
        file=sys.stderr,
    )
    raise SystemExit(1)

first_bad = None
checked = []
drift_commits = []
for entry in commits:
    if (time.time() - start) > max_seconds:
        print("[bisect][error] synthetic run exceeded --max-seconds budget", file=sys.stderr)
        raise SystemExit(1)
    sha = entry.get("sha")
    verdict = entry.get("verdict")
    if not isinstance(sha, str) or verdict not in ("good", "drift", "bad"):
        print(f"[bisect][error] bad synthetic entry: {entry!r}", file=sys.stderr)
        raise SystemExit(1)
    checked.append({"sha": sha, "verdict": verdict})
    if verdict == "drift":
        drift_commits.append(sha)
    if verdict == "bad":
        first_bad = sha
        break

if not first_bad:
    print("[bisect][error] no bad commit in synthetic map", file=sys.stderr)
    raise SystemExit(1)

summary = {
    "mode": "synthetic",
    "first_bad_commit": first_bad,
    "drift_commits": drift_commits,
    "checked_commits": checked,
    "max_commits": max_commits,
    "max_seconds": max_seconds,
}
if report_json:
    with open(report_json, "w", encoding="utf-8") as f:
        json.dump(summary, f, sort_keys=True)
print(f"[bisect][ok] synthetic first bad commit: {first_bad}")
raise SystemExit(0)
PY
  exit $?
fi

log "[bisect] start good=$good_bundle bad=$bad_bundle max_commits=$max_commits max_seconds=$max_seconds per_replay=$per_replay_seconds"
log "[bisect] using evidence_bin=$nebin"
log "[bisect] using replay_worktree=$worktree_dir"
log "[bisect] verifying input bundles"
env -u NEXUS_EVIDENCE_CI_PUBKEY -u NEXUS_EVIDENCE_BRINGUP_PUBKEY \
  NEXUS_EVIDENCE_BIN="$nebin" \
  "$VERIFY_SCRIPT" "$good_bundle" --policy=any >/dev/null
env -u NEXUS_EVIDENCE_CI_PUBKEY -u NEXUS_EVIDENCE_BRINGUP_PUBKEY \
  NEXUS_EVIDENCE_BIN="$nebin" \
  "$VERIFY_SCRIPT" "$bad_bundle" --policy=any >/dev/null

meta_json="$tmpdir/meta.json"
python3 - "$good_bundle" "$bad_bundle" >"$meta_json" <<'PY'
import json
import subprocess
import sys

good, bad = sys.argv[1:]

def read_config(bundle):
    cp = subprocess.run(
        ["tar", "-xOf", bundle, "config.json"],
        capture_output=True,
        text=True,
        check=False,
    )
    if cp.returncode != 0:
        raise RuntimeError(f"bundle missing config.json: {bundle}")
    cfg = json.loads(cp.stdout)
    profile = cfg.get("profile", "")
    sha = cfg.get("build_sha", "")
    if not profile or not sha:
        raise RuntimeError(f"bundle config missing profile/build_sha: {bundle}")
    return profile, sha

gp, gs = read_config(good)
bp, bs = read_config(bad)
if gp != bp:
    raise RuntimeError(f"profile mismatch: good={gp} bad={bp}")
print(json.dumps({
    "profile": gp,
    "good_sha": gs,
    "bad_sha": bs
}, sort_keys=True))
PY

profile=$(python3 - "$meta_json" <<'PY'
import json,sys
print(json.load(open(sys.argv[1]))["profile"])
PY
)
good_sha=$(python3 - "$meta_json" <<'PY'
import json,sys
print(json.load(open(sys.argv[1]))["good_sha"])
PY
)
bad_sha=$(python3 - "$meta_json" <<'PY'
import json,sys
print(json.load(open(sys.argv[1]))["bad_sha"])
PY
)

git -C "$ROOT" cat-file -e "${good_sha}^{commit}" || die "good SHA not present in repo: $good_sha"
git -C "$ROOT" cat-file -e "${bad_sha}^{commit}" || die "bad SHA not present in repo: $bad_sha"

mapfile -t commits < <(git -C "$ROOT" rev-list --reverse "${good_sha}..${bad_sha}")
(( ${#commits[@]} > 0 )) || die "empty range between good_sha=$good_sha and bad_sha=$bad_sha"
(( ${#commits[@]} <= max_commits )) || die "commit range ${#commits[@]} exceeds --max-commits=$max_commits"
log "[bisect] commit-range size=${#commits[@]} profile=$profile good_sha=$good_sha bad_sha=$bad_sha"

start_epoch=$(date +%s)
first_bad=""
checked=0
checked_json="$tmpdir/checked.jsonl"
low=0
high=$(( ${#commits[@]} - 1 ))
search_strategy="binary"

while (( low <= high )); do
  now_epoch=$(date +%s)
  elapsed=$(( now_epoch - start_epoch ))
  remaining=$(( max_seconds - elapsed ))
  if (( remaining <= 0 )); then
    die "bisect budget exhausted (--max-seconds=$max_seconds)"
  fi
  per_run="$per_replay_seconds"
  if (( remaining < per_run )); then
    per_run="$remaining"
  fi

  mid=$(( (low + high) / 2 ))
  sha="${commits[$mid]}"
  checked=$((checked + 1))
  log "[bisect] check #$checked sha=$sha interval=[$low,$high] mid=$mid budget=${per_run}s"

  replay_report="$tmpdir/replay-$sha.json"
  replay_log="$tmpdir/replay-$sha.log"
  if "$REPLAY_SCRIPT" "$good_bundle" \
      --git-sha="$sha" \
      --max-seconds="$per_run" \
      --report-json="$replay_report" \
      --log-file="$replay_log" \
      --worktree-dir="$worktree_dir"; then
    verdict="good"
  else
    verdict="bad"
  fi
  log "[bisect] verdict sha=$sha verdict=$verdict replay_log=$replay_log"
  printf '{"sha":"%s","verdict":"%s"}\n' "$sha" "$verdict" >>"$checked_json"

  if [[ "$verdict" == "good" ]]; then
    low=$((mid + 1))
  else
    first_bad="$sha"
    high=$((mid - 1))
  fi
done

[[ -n "$first_bad" ]] || die "no first bad commit found within checked range"

summary_path="$tmpdir/bisect-summary.json"
python3 - "$summary_path" "$profile" "$good_sha" "$bad_sha" "$first_bad" "$max_commits" "$max_seconds" "$checked_json" "${#commits[@]}" <<'PY'
import json
import sys

(
    out_path,
    profile,
    good_sha,
    bad_sha,
    first_bad,
    max_commits,
    max_seconds,
    checked_path,
    range_commit_count,
) = sys.argv[1:]

checked = []
with open(checked_path, "r", encoding="utf-8") as f:
    for line in f:
        checked.append(json.loads(line))

summary = {
    "mode": "replay",
    "search_strategy": "binary",
    "profile": profile,
    "good_sha": good_sha,
    "bad_sha": bad_sha,
    "first_bad_commit": first_bad,
    "range_commit_count": int(range_commit_count),
    "max_commits": int(max_commits),
    "max_seconds": int(max_seconds),
    "checked_commits": checked,
}
with open(out_path, "w", encoding="utf-8") as f:
    json.dump(summary, f, sort_keys=True)
print(json.dumps(summary, sort_keys=True))
PY

if [[ -n "$report_json" ]]; then
  cp "$summary_path" "$report_json"
fi

log "[bisect] ok first_bad_commit=$first_bad"
log "[bisect] summary path: $summary_path"
log "[bisect] report path: ${report_json:-<not-written>}"

