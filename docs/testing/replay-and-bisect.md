---
schema: 1
title: "Replay and bisect"
role: reference
subsystem: devx
sources: [tools/replay-evidence.sh, tools/diff-traces.sh, tools/bisect-evidence.sh, scripts/regression-bisect.sh]
contracts: [RFC-0038]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Replay and bisect

> **Purpose** — replay a sealed evidence bundle, classify trace drift, and bisect a regression between two bundles, with the budgets and allowlist that bound them.
> **Owns** — `tools/replay-evidence.sh`, `tools/diff-traces.sh`, `tools/bisect-evidence.sh`, `scripts/regression-bisect.sh`
> **Contracts** — [RFC-0038](../rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md) § Phase 6 (replay capability, cross-host determinism floor)
> **Invariants** — every replay and bisect carries an explicit time budget. Config drift is allowed only for the fields listed below; the list only grows by review. A replay never accepts run-control env from the caller.
> **Read next** — [evidence-bundle.md](evidence-bundle.md), [run-logs.md](run-logs.md)

## Replay

```bash
tools/replay-evidence.sh target/evidence/<bundle>.tar.gz --max-seconds=300 \
  [--git-sha=<sha>] [--report-json=<path>] [--log-file=<path>] [--worktree-dir=<path>]
```

1. Verify the signature (`tools/verify-evidence.sh --policy=any`).
2. Read `profile`, `build_sha`, `kernel_cmdline` and env from `config.json`.
3. Check out a detached worktree at that SHA (default `target/replay-worktree`, reused).
4. Run `just test-os <profile>` with the recorded env under the hard timeout.
5. Extract the replay `trace.jsonl` and diff it against the bundle's (`tools/diff-traces.sh`).
6. Compare configs against the allowlist.

Exit 0 = exact trace match and only allowlisted config drift. The cargo cache is
`target/replay-cache` (`REPLAY_CARGO_TARGET_DIR`); replaying a SHA already built sets
`NEXUS_SKIP_BUILD=1`. Use `--worktree-dir` for parallel sessions.

## Trace diff

```bash
tools/diff-traces.sh original-trace.jsonl replay-trace.jsonl --format=lines|json
```

Input: each non-empty line is a JSON object with string `marker` and `phase`; other fields are
ignored. Invalid JSON or a missing field is a usage error.

```json
{
  "status": "exact_match | diff",
  "classes": ["..."],
  "counts": { "original_entries": 0, "replay_entries": 0 },
  "details": { "extra_marker": [], "missing_marker": [], "phase_mismatch": [], "reorder": [] }
}
```

| Class | Condition | Detail item |
|---|---|---|
| `exact_match` | the `(marker, phase)` sequences are identical | — |
| `extra_marker` | replay has a tuple more often than original | `marker`, `phase`, `count` |
| `missing_marker` | original has a tuple more often than replay | `marker`, `phase`, `count` |
| `reorder` | equal multisets, different order | `index`, `original`, `replay` |
| `phase_mismatch` | same marker at an index, different phase | `index`, `marker`, `original_phase`, `replay_phase` |

| Exit | Meaning |
|---|---|
| 0 | exact match |
| 1 | classified diff |
| 2 | usage or parse error |

Fixtures: `docs/testing/trace-diff-fixtures.json` — `cases[]` with `name`, `original`, `replay`,
`expect_exit`, `expect_classes`, one case per class. Check them without QEMU:

```bash
python3 - <<'PY'
import json, pathlib, subprocess, tempfile
cases = json.loads(pathlib.Path("docs/testing/trace-diff-fixtures.json").read_text())["cases"]
with tempfile.TemporaryDirectory() as td:
    for i, c in enumerate(cases):
        o, r = pathlib.Path(td, f"{i}-o.jsonl"), pathlib.Path(td, f"{i}-r.jsonl")
        o.write_text("".join(json.dumps(x, sort_keys=True) + "\n" for x in c["original"]))
        r.write_text("".join(json.dumps(x, sort_keys=True) + "\n" for x in c["replay"]))
        cp = subprocess.run(["tools/diff-traces.sh", str(o), str(r), "--format=json"], capture_output=True, text=True)
        assert cp.returncode == c["expect_exit"], c["name"]
        assert sorted(json.loads(cp.stdout)["classes"]) == sorted(c["expect_classes"]), c["name"]
print("trace-diff fixtures: ok")
PY
```

## Bisect

```bash
tools/bisect-evidence.sh target/evidence/<good>.tar.gz target/evidence/<bad>.tar.gz \
  --max-commits=64 --max-seconds=1800 [--per-replay-seconds=300] [--report-json=<path>]
```

Verifies both bundles, takes `good_sha..bad_sha`, rejects a range above `--max-commits` and a
total run above `--max-seconds`, then binary-searches (assumes good → bad is monotonic), replaying
each probe against the good bundle's trace, and reports the first diverging commit.

- `--synthetic-map=<json>` runs the algorithm without QEMU from a `{"commits": [{"sha", "verdict":
  "good|drift|bad"}]}` map; `drift` counts as good and is listed separately. Fixture:
  `docs/testing/bisect-good-drift-regress.json` (expects `first_bad_commit: c2cccccc`,
  `drift_commits: [c1bbbbbb]`).
- `scripts/regression-bisect.sh --good-bundle=<p> --bad-bundle=<p> --max-commits=<n>
  --max-seconds=<n>` wraps it for manual use; it is not wired into CI.

## Config allowlist

Allowed drift in `config.json`:

| Field | When |
|---|---|
| `wall_clock_utc`, `qemu_version`, `host_info` | always |
| `build_sha` | only with `--git-sha=<override>` |
| env key `HOSTNAME` | always |

Everything else fails closed. Adding an entry needs, in one change: a row here, the matching
`allow_top`/`allow_env` entry in `tools/replay-evidence.sh`, and a review note on why the field is
nondeterministic but behavior-irrelevant. Removing entries is always allowed.

## Hard gates

- Replay without `--max-seconds`, bisect without `--max-commits` or `--max-seconds`: rejected.
- Replay fails closed when the caller sets `PROFILE`, `SELFTEST_PROFILE`, `RUN_PHASE`,
  `REQUIRE_SMP`, `REQUIRE_DSOFTBUS`, `REQUIRE_QEMU_DHCP`, `REQUIRE_QEMU_DHCP_STRICT` or `KERNEL_CMDLINE`.
- Replay fails closed on a bundle with non-empty `qemu_args` (not reconstructed).

## Cross-host floor

The determinism floor of RFC-0038: the same sealed bundle replayed on a CI runner and on a
developer host must both report an exact trace match and only allowlisted config drift. Run
`tools/replay-evidence.sh <bundle> --max-seconds=300 --report-json=<host>.json` on each and
require `trace_diff.status == "exact_match"` and `config_compare.ok == true` in both reports.
Any other result is a regression candidate.

A tamper drill: unpack a bundle, insert one line into `trace.jsonl`, repack the six entries
without a leading `./`, reseal (`tools/seal-evidence.sh … --label=bringup`) and replay — the replay
must exit 1 with `missing_marker`, proving the classifier catches what the signature no longer does.
