# Bring-up ledger template (one per driver / board / SoC block)

Copy into `tasks/TASK-XXXX-<driver>-bringup.md` (front matter as in `tasks/TASK-TEMPLATE.md`)
and fill every heading with measurements, not intentions. Four phases, each with its
evidence path; the Definition of Done is the last heading and is copied, not paraphrased.

```markdown
## Context

- The device: tree node(s) with `compatible`, `reg`, clocks, resets, power domain,
  interrupts, `dma-ranges` of its bus — as the STOCK tree names them
  (`docs/board/measurements/<date>-<topic>/dt-*.txt`).
- What our chain does today when the device is absent (`init: <plane> none`, the FAIL
  markers tolerated in `config/fail-marker-allow-board.txt` with this task as reference).
- What must not change: the marker contracts, the slot declarations, the tree golden.

## P0 — Measurement (docs/board/measurements/<date>-<topic>/README.md)

Question → Instrument → Results → Verdict. The verdict names the decisions the code
takes: which clocks/resets/domain are real, where the buffers live (bus address, reach,
coherence), which registers the first-light sequence writes and in which order, what the
vendor driver does NOT touch. Raw dumps and the annotated dump archived in the folder.

## P1 — Model (host)

`source/.../model/` or a `dc`/`hw` module with a register-writer trait: the P0 dump is a
golden (the model reproduces the measured words for the measured mode, modulo buffer
base). `test_reject_*` for every bound the driver enforces on untrusted input (EDID,
descriptors, sizes). No OS code yet.

## P2 — First light (board)

The driver in its service, the grants from init's tree walk, the socd bring-up steps for
the block (RFC-0106), the marker `<svc>: <thing> ok (<measured value>)` declared and
REQUIRED in `scripts/board-test.sh`'s ladder (or the QEMU lane's `expected_sequence`).
Cycle protocol per hypothesis: what decides it BEFORE the flash. Archive the first green
transcript next to P0.

## P3 — Gate sweep

- The gate shown red once (the version before the fix, or an injected failure) — transcript
  archived.
- Tolerated reds this driver closes are removed from `config/fail-marker-allow-board.txt`.
- The lane in `test-all` (`scripts/check-ci-parity.sh` keeps it reachable); the tree golden
  regenerated if the node changed; retired names for anything this driver replaced.
- Numbers in the ledger: bring-up time, present/transfer latency, memory (`mm snapshot`).

## Definition of Done

1. The measurement folder exists and answers its question with archived raw data.
2. The host model reproduces the measured registers for the measured mode (goldens).
3. The lane is green on its REQUIRED ladder with no tolerated red that lacks a reference.
4. The gate has been shown red once on a real failure, with the transcript archived.
```
