<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Replay And Bisect (Phase 6)

- Status: `TASK-0023B` Phase 6 (`P6-01`..`P6-06`)
- Tools:
  - [`tools/replay-evidence.sh`](../../tools/replay-evidence.sh)
  - [`tools/diff-traces.sh`](../../tools/diff-traces.sh)
  - [`tools/bisect-evidence.sh`](../../tools/bisect-evidence.sh)
  - [`scripts/regression-bisect.sh`](../../scripts/regression-bisect.sh)

This is the operational guide for deterministic replay and bounded regression bisect.

## 1. Replay Workflow

Replay reproduces one sealed bundle on a pinned git SHA and compares traces.

```bash
tools/replay-evidence.sh target/evidence/<bundle>.tar.gz --max-seconds=300
```

Replay steps:

1. Verify bundle signature (`tools/verify-evidence.sh --policy=any`).
2. Read recorded `profile`, `build_sha`, and replay env from bundle `config.json`.
3. Create detached git worktree at the pinned SHA.
4. Run `just test-os <profile>` with recorded env under a hard timeout.
5. Extract replay `trace.jsonl`.
6. Diff original vs replay trace using `tools/diff-traces.sh`.
7. Enforce config drift allowlist (section 4).

Exit `0` means exact trace replay and allowlist-compliant config drift only.

### Performance notes (production-grade defaults)

- Replay uses a persistent cargo cache at `target/replay-cache`.
- Replay reuses a persistent detached worktree (`target/replay-worktree` by default).
- If the same SHA is replayed again, replay sets `NEXUS_SKIP_BUILD=1` automatically and runs QEMU against already-built artifacts.
- Use `--worktree-dir=<path>` to isolate parallel replay sessions.

## 2. Trace Diff Workflow

Classify trace drift directly:

```bash
tools/diff-traces.sh original-trace.jsonl replay-trace.jsonl --format=lines
tools/diff-traces.sh original-trace.jsonl replay-trace.jsonl --format=json
```

Format and classes are defined in:

- [`docs/testing/trace-diff-format.md`](trace-diff-format.md)

## 3. Bounded Bisect Workflow

Find first regressing commit between a last-green and failed bundle.

```bash
tools/bisect-evidence.sh \
  target/evidence/<good>.tar.gz \
  target/evidence/<bad>.tar.gz \
  --max-commits=64 \
  --max-seconds=1800 \
  --per-replay-seconds=300
```

Behavior:

- verifies both bundles,
- resolves `good_sha..bad_sha` commit range,
- rejects ranges larger than `--max-commits`,
- rejects wallclock overrun beyond `--max-seconds`,
- probes the range with **binary search** (monotonic-good-to-bad assumption),
- replays each probe commit against the good-bundle trace contract,
- reports first commit whose replay diverges.

CI wrapper:

```bash
scripts/regression-bisect.sh \
  --good-bundle=target/evidence/<good>.tar.gz \
  --bad-bundle=target/evidence/<bad>.tar.gz \
  --max-commits=64 \
  --max-seconds=1800
```

## 4. Determinism Allowlist (Append-Only)

Replay allows config drift only for host-surface fields that do not alter behavior proof.

Allowed top-level `config.json` drift:

- `wall_clock_utc`
- `qemu_version`
- `host_info`

Conditionally allowed:

- `build_sha` (only when replay is explicitly forced to `--git-sha=<override>`)

Allowed env-key drift:

- `HOSTNAME`

Everything else in config comparison is fail-closed.

### Append-only rule

This allowlist is append-only.

Any addition requires:

1. update in this document (explicit bullet),
2. matching code change in replay enforcement logic,
3. reviewer signoff documenting why the field is nondeterministic but behavior-irrelevant.

Removing allowlist entries is allowed (tightening).

## 5. Hard Gates

- Replay without explicit `--max-seconds` is rejected.
- Bisect without explicit `--max-commits` or `--max-seconds` is rejected.
- Replay fails closed when external run-control env overrides are present (`PROFILE`, `SELFTEST_PROFILE`, `RUN_PHASE`, `REQUIRE_*`, `KERNEL_CMDLINE`).
- Replay fails closed if bundle requires unsupported runtime reconstruction (for the P6 skeleton: non-empty recorded `qemu_args`).

## 6. Cross-Host Floor Procedure

For one sealed bundle:

1. CI runner executes replay (`--max-seconds=300`) and stores summary.
2. One dev host executes replay on the same bundle and stores summary.
3. Both must report:
   - exact trace match,
   - config drift only in allowlisted fields.

If either host reports non-empty trace diff or disallowed config drift, treat as regression candidate.

## 7. Current Validation Record (2026-04-20)

Reference bundle used:

- `target/evidence/20260420T133203Z-full-b84e4c2.tar.gz` (sealed)

Replays executed:

- Dev host (native): `tools/replay-evidence.sh ... --report-json=.cursor/replay-dev-a.json` -> exact trace match, allowlist-compliant config compare.
- Dev host (containerized CI-like env): `podman run ... tools/replay-evidence.sh ... --worktree-dir=target/replay-worktree-container --report-json=.cursor/replay-ci-like.json` -> exact trace match, allowlist-compliant config compare.

Open closure item:

- RFC/task contract names "CI runner + one dev host" explicitly. The second run above is containerized on the same machine, so it is a strong floor check but not yet an external CI-runner artifact.

## 8. CI Runner Copy/Paste Recipe (P6-05 Closure)

Use the same sealed bundle on both hosts.

### CI runner step

```bash
set -euo pipefail
BUNDLE="target/evidence/20260420T133203Z-full-b84e4c2.tar.gz"
REPORT=".cursor/replay-ci.json"
LOG=".cursor/replay-ci.log"

tools/replay-evidence.sh "$BUNDLE" \
  --max-seconds=300 \
  --log-file="$LOG" \
  --report-json="$REPORT"

python3 - <<'PY'
import json
from pathlib import Path
p = Path(".cursor/replay-ci.json")
obj = json.loads(p.read_text(encoding="utf-8"))
assert obj["trace_diff"]["status"] == "exact_match", obj["trace_diff"]
assert obj["config_compare"]["ok"] is True, obj["config_compare"]
print("ci replay: exact_match + allowlist_ok")
PY
```

### Dev host step

```bash
set -euo pipefail
BUNDLE="target/evidence/20260420T133203Z-full-b84e4c2.tar.gz"
REPORT=".cursor/replay-dev.json"
LOG=".cursor/replay-dev.log"

tools/replay-evidence.sh "$BUNDLE" \
  --max-seconds=300 \
  --log-file="$LOG" \
  --report-json="$REPORT"

python3 - <<'PY'
import json
from pathlib import Path
p = Path(".cursor/replay-dev.json")
obj = json.loads(p.read_text(encoding="utf-8"))
assert obj["trace_diff"]["status"] == "exact_match", obj["trace_diff"]
assert obj["config_compare"]["ok"] is True, obj["config_compare"]
print("dev replay: exact_match + allowlist_ok")
PY
```

### Final compare gate

```bash
python3 - <<'PY'
import json
from pathlib import Path
ci = json.loads(Path(".cursor/replay-ci.json").read_text(encoding="utf-8"))
dev = json.loads(Path(".cursor/replay-dev.json").read_text(encoding="utf-8"))
for name, obj in [("ci", ci), ("dev", dev)]:
    assert obj["trace_diff"]["status"] == "exact_match", (name, obj["trace_diff"])
    assert obj["config_compare"]["ok"] is True, (name, obj["config_compare"])
print("P6-05 cross-host floor: PASS (ci + dev exact match)")
PY
```

Archive these four artifacts in CI:

- `.cursor/replay-ci.json`
- `.cursor/replay-ci.log`
- `.cursor/replay-dev.json` (copied from dev host evidence exchange)
- `.cursor/replay-dev.log` (optional but recommended for triage)

## 9. Phase-6 Proof Floor — current evidence map

All Phase-6 proof-floor items + hard gates are verified locally with reproducible artifacts. Run any of the commands below to re-verify on a fresh checkout.

| Floor item | Command (host-runnable) | Evidence path | Status |
| --- | --- | --- | --- |
| Empty diff against good bundle | `tools/replay-evidence.sh target/evidence/20260420T133203Z-full-b84e4c2.tar.gz --max-seconds=300 --report-json=.cursor/replay-dev-a.json` | `.cursor/replay-dev-a.json` (`trace_diff.status == "exact_match"`) | ✅ pass |
| Synthetic bad-bundle classified diff | See §10 below | `.cursor/replay-synthetic-bad.{log,json}` (`status: "diff", classes: ["missing_marker"]`, exit 1) | ✅ pass |
| 3-commit good→drift→regress bisect | `tools/bisect-evidence.sh ... --max-seconds=30 --max-commits=3 --synthetic-map=docs/testing/bisect-good-drift-regress.json --report-json=.cursor/bisect-good-drift-regress.json` | `.cursor/bisect-good-drift-regress.json` (`first_bad_commit: c2cccccc`, `drift_commits: [c1bbbbbb]`) | ✅ pass |
| Performance floor (warm replay) | `time tools/replay-evidence.sh ... --max-seconds=300` (run twice) | log shows `NEXUS_SKIP_BUILD=1` on second run, ~14s vs ~67s | ✅ pass |
| Hard gate: `--max-seconds` mandatory | `tools/replay-evidence.sh target/evidence/<bundle>` | exit 2 with usage | ✅ pass |
| Hard gate: env override rejected | `PROFILE=full tools/replay-evidence.sh ... --max-seconds=30` | exit 1 with `[replay][error] environment override PROFILE is set` | ✅ pass |
| Hard gate: `--max-commits` mandatory | `tools/bisect-evidence.sh <good> <bad> --max-seconds=30` | exit 1 with `--max-commits is mandatory` | ✅ pass |
| Cross-host floor (native + container, same machine) | replay run on dev host + `podman` CI-like profile | `.cursor/replay-dev-a.json`, `.cursor/replay-ci-like.json` | ✅ partial (see §11) |

## 10. Reproducing the synthetic bad-bundle proof

```bash
set -euo pipefail
mkdir -p target/evidence/synthetic-bad
( cd target/evidence/synthetic-bad && rm -rf work && mkdir work \
    && cd work && tar -xzf ../../20260420T133203Z-full-b84e4c2.tar.gz )

# Inject one synthetic marker so the recorded trace differs from a fresh replay
sed -i.bak '5a {"marker":"SYNTHETIC: tamper probe","phase":"bringup","ts_ms_from_boot":null,"profile":"full"}' \
  target/evidence/synthetic-bad/work/trace.jsonl
rm target/evidence/synthetic-bad/work/trace.jsonl.bak

# Repack without leading "./" entry (bundle canonicalization rejects it)
( cd target/evidence/synthetic-bad/work \
    && tar -czf ../corrupt-bundle.tar.gz \
       config.json manifest.tar meta.json signature.bin trace.jsonl uart.log )

# Re-seal so signature verification passes — diff classifier MUST still catch the tamper
NEXUS_EVIDENCE_BIN=target/debug/nexus-evidence \
  tools/seal-evidence.sh target/evidence/synthetic-bad/corrupt-bundle.tar.gz --label=bringup

# Replay — expected: exit 1, classified diff with the synthetic marker as missing_marker
tools/replay-evidence.sh target/evidence/synthetic-bad/corrupt-bundle.tar.gz \
  --max-seconds=300 \
  --log-file=.cursor/replay-synthetic-bad.log \
  --report-json=.cursor/replay-synthetic-bad.json
```

## 11. Remaining environmental item (not a code or design gap)

The RFC-0038 wording for the cross-host floor is `CI runner + one dev host`. The two existing exact-match runs (`.cursor/replay-dev-a.json`, `.cursor/replay-ci-like.json`) are both on the same physical machine. Capturing the third artifact is purely an execution step on the project CI runner using the recipe in §7/§8 — there is no remaining tool, fixture, or doc work for Phase 6.

When the CI run lands, the closure flip is exactly:

- archive `.cursor/replay-ci.json` + `.cursor/replay-ci.log`
- update `tasks/TASK-0023B-...md` P6-05 line: drop `⚠ external CI-runner artifact pending`, mark `✅ done`
- tick the Phase-6 checkbox in `docs/rfcs/RFC-0038-...md`
- mirror status in `tasks/STATUS-BOARD.md`, `tasks/IMPLEMENTATION-ORDER.md`, `.cursor/{current_state,handoff/current,next_task_prep}.md`
