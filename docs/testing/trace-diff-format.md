<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Trace Diff Format (Phase 6)

- Status: `TASK-0023B` Phase 6 / Cut `P6-02`
- Tool: [`tools/diff-traces.sh`](../../tools/diff-traces.sh)
- Input: two `trace.jsonl` files (`original`, `replay`)

This document defines the deterministic diff output used by replay and bisect tooling.

## Purpose

`trace.jsonl` is the behavior-level proof projection extracted from UART by `nexus-evidence`.
`diff-traces.sh` classifies replay drift into stable classes so operators can distinguish:

- exact replay,
- missing/extra markers,
- ordering drift,
- phase-mapping drift.

## Input Contract

Each non-empty line in both files must be a JSON object with:

- `marker` (string)
- `phase` (string)

Other fields are ignored by the classifier.

Invalid JSON or missing/non-string `marker`/`phase` is a hard usage error (exit code `2`).

## Output Contract

`tools/diff-traces.sh` supports:

- `--format=lines` (human-readable)
- `--format=json` (machine-readable)

### JSON shape

```json
{
  "status": "exact_match | diff",
  "classes": ["..."],
  "counts": {
    "original_entries": 0,
    "replay_entries": 0
  },
  "details": {
    "extra_marker": [],
    "missing_marker": [],
    "phase_mismatch": [],
    "reorder": []
  }
}
```

## Classification Rules

### `exact_match`

- Condition: original and replay sequences are byte-identical as `(marker, phase)` tuples.
- Exit code: `0`.

### `extra_marker`

- Condition: replay contains tuples that original does not contain (or contains at higher multiplicity).
- Details item shape:
  - `marker`, `phase`, `count`.

### `missing_marker`

- Condition: original contains tuples that replay does not contain (or contains at lower multiplicity).
- Details item shape:
  - `marker`, `phase`, `count`.

### `reorder`

- Condition: tuple multisets are equal, but sequence order differs.
- Details item shape:
  - `index`, `original`, `replay` (both objects with `marker` and `phase`).

### `phase_mismatch`

- Condition: index-wise marker identity is unchanged (`marker` equal), but `phase` differs at that index.
- Details item shape:
  - `index`, `marker`, `original_phase`, `replay_phase`.

## Exit Codes

- `0`: exact match.
- `1`: non-empty classified diff (`extra_marker`, `missing_marker`, `reorder`, and/or `phase_mismatch`).
- `2`: usage/input parse failure.

## Unit Fixture Set

Fixture definitions:

- [`docs/testing/trace-diff-fixtures.json`](trace-diff-fixtures.json)

Fixture runner command (deterministic, no QEMU):

```bash
python3 - <<'PY'
import json, pathlib, subprocess, tempfile
root = pathlib.Path(".")
cases = json.loads((root / "docs/testing/trace-diff-fixtures.json").read_text())["cases"]
tool = root / "tools/diff-traces.sh"
with tempfile.TemporaryDirectory() as td:
    td = pathlib.Path(td)
    for i, case in enumerate(cases):
        o = td / f"{i}-o.jsonl"; r = td / f"{i}-r.jsonl"
        o.write_text("".join(json.dumps(x, sort_keys=True)+"\n" for x in case["original"]))
        r.write_text("".join(json.dumps(x, sort_keys=True)+"\n" for x in case["replay"]))
        cp = subprocess.run([str(tool), str(o), str(r), "--format=json"], capture_output=True, text=True)
        got = sorted(json.loads(cp.stdout)["classes"])
        assert cp.returncode == case["expect_exit"], (case["name"], cp.returncode, case["expect_exit"])
        assert got == sorted(case["expect_classes"]), (case["name"], got, case["expect_classes"])
print("trace-diff fixtures: ok")
PY
```

Covered classes:

- exact match,
- extra marker,
- missing marker,
- reorder,
- phase mismatch.

