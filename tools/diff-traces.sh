#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: Deterministic trace diff classifier for evidence replay (P6-02).
# Compares two trace.jsonl files (original vs replay) and classifies drift:
#   - exact_match
#   - extra_marker
#   - missing_marker
#   - reorder
#   - phase_mismatch
#
# OWNERS: @runtime
# STATUS: Functional (TASK-0023B P6-02)

set -Eeuo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/diff-traces.sh <original-trace.jsonl> <replay-trace.jsonl> [--format=lines|json]

Exit codes:
  0  exact_match
  1  non-empty classified diff
  2  usage / input parse error
EOF
  exit 2
}

orig=""
replay=""
format="lines"

for arg in "$@"; do
  case "$arg" in
    --format=*) format="${arg#--format=}" ;;
    -h|--help) usage ;;
    -*)
      echo "[trace-diff][error] unknown flag: $arg" >&2
      usage
      ;;
    *)
      if [[ -z "$orig" ]]; then
        orig="$arg"
      elif [[ -z "$replay" ]]; then
        replay="$arg"
      else
        echo "[trace-diff][error] only two positional arguments are accepted" >&2
        usage
      fi
      ;;
  esac
done

[[ -n "$orig" && -n "$replay" ]] || usage
[[ -f "$orig" ]] || { echo "[trace-diff][error] original trace not found: $orig" >&2; exit 2; }
[[ -f "$replay" ]] || { echo "[trace-diff][error] replay trace not found: $replay" >&2; exit 2; }
[[ "$format" == "lines" || "$format" == "json" ]] || {
  echo "[trace-diff][error] --format must be lines|json, got: $format" >&2
  exit 2
}

python3 - "$orig" "$replay" "$format" <<'PY'
import json
import sys
from collections import Counter

orig_path, replay_path, out_format = sys.argv[1:]


def load_trace(path: str):
    out = []
    with open(path, "r", encoding="utf-8") as f:
        for idx, raw in enumerate(f, start=1):
            line = raw.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError as e:
                raise ValueError(f"{path}: line {idx}: invalid JSON: {e}") from e
            marker = obj.get("marker")
            phase = obj.get("phase")
            if not isinstance(marker, str) or not isinstance(phase, str):
                raise ValueError(
                    f"{path}: line {idx}: marker/phase must be strings (got marker={type(marker).__name__}, phase={type(phase).__name__})"
                )
            out.append((marker, phase))
    return out


def classify(orig, replay):
    result = {
        "status": "exact_match",
        "classes": [],
        "counts": {
            "original_entries": len(orig),
            "replay_entries": len(replay),
        },
        "details": {
            "extra_marker": [],
            "missing_marker": [],
            "phase_mismatch": [],
            "reorder": [],
        },
    }

    if orig == replay:
        result["classes"] = ["exact_match"]
        return result

    c_orig = Counter(orig)
    c_replay = Counter(replay)

    extras = []
    for pair in sorted(c_replay):
        delta = c_replay[pair] - c_orig.get(pair, 0)
        if delta > 0:
            extras.append({"marker": pair[0], "phase": pair[1], "count": delta})
    if extras:
        result["classes"].append("extra_marker")
        result["details"]["extra_marker"] = extras

    missing = []
    for pair in sorted(c_orig):
        delta = c_orig[pair] - c_replay.get(pair, 0)
        if delta > 0:
            missing.append({"marker": pair[0], "phase": pair[1], "count": delta})
    if missing:
        result["classes"].append("missing_marker")
        result["details"]["missing_marker"] = missing

    # phase_mismatch is detected index-wise when marker identity is stable but
    # phase differs. This catches "same marker moved to wrong phase mapping".
    phase_mismatch = []
    lim = min(len(orig), len(replay))
    for i in range(lim):
        om, op = orig[i]
        rm, rp = replay[i]
        if om == rm and op != rp:
            phase_mismatch.append(
                {
                    "index": i,
                    "marker": om,
                    "original_phase": op,
                    "replay_phase": rp,
                }
            )
    if phase_mismatch:
        result["classes"].append("phase_mismatch")
        result["details"]["phase_mismatch"] = phase_mismatch

    # Reorder is only meaningful if the multiset is equal but sequence differs.
    if c_orig == c_replay and orig != replay:
        mismatches = []
        for i in range(lim):
            if orig[i] != replay[i]:
                mismatches.append(
                    {
                        "index": i,
                        "original": {"marker": orig[i][0], "phase": orig[i][1]},
                        "replay": {"marker": replay[i][0], "phase": replay[i][1]},
                    }
                )
                if len(mismatches) >= 20:
                    break
        result["classes"].append("reorder")
        result["details"]["reorder"] = mismatches

    if not result["classes"]:
        # Defensive fallback: any non-exact diff must have at least one class.
        result["classes"] = ["reorder"]

    result["status"] = "diff"
    return result


try:
    orig = load_trace(orig_path)
    replay = load_trace(replay_path)
except ValueError as e:
    print(f"[trace-diff][error] {e}", file=sys.stderr)
    sys.exit(2)

summary = classify(orig, replay)
ok = summary["classes"] == ["exact_match"]

if out_format == "json":
    print(json.dumps(summary, sort_keys=True))
else:
    if ok:
        print("[trace-diff] exact_match")
    else:
        classes = ",".join(summary["classes"])
        print(f"[trace-diff] status=diff classes={classes}")
        print(f"[trace-diff] counts original={summary['counts']['original_entries']} replay={summary['counts']['replay_entries']}")
        for cls in ("extra_marker", "missing_marker", "phase_mismatch", "reorder"):
            entries = summary["details"][cls]
            if not entries:
                continue
            print(f"[trace-diff] {cls}:")
            for item in entries[:10]:
                print(f"  - {json.dumps(item, sort_keys=True)}")

sys.exit(0 if ok else 1)
PY

