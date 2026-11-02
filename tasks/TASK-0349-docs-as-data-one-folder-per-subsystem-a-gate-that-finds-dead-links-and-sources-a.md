---
schema: 1
id: TASK-0349
title: "Docs as data: one folder per subsystem, a gate that finds dead links and sources, and a drift report"
status: in_progress
status_note: "built 2026-11-02; closes when its consumer TASK-0347/P4 runs the docs sweep with nx ledger stale"
owner: ["@tools-team"]
created: 2026-11-02
updated: 2026-11-02
subsystem: devx
kind: docs
novelty: variant
estimate: 2
started: 2026-11-02
active_days: 1
dates_source: recorded
variance_note: "estimate 2d, built in 1: the frontmatter/lint machinery of RFC-0115 was reused, the rewrite ran as 8 parallel passes"
goal:
  metric: "words an agent reads in docs/ outside rfcs, adr, board, dev; dead doc links in code and docs/"
  baseline: "105k words; 226 dead doc links (14 in code)"
  target: "<=50k words; 0 dead links in code and docs/"
  instrument: "find docs -name '*.md' | wc -w; nx ledger lint"
  result: "48.9k words (target <=50k); 0 dead doc links in code and docs/; 223 grandfathered notes in planning docs"
consumer: TASK-0347/P4
implements: [RFC-0115]
phases:
  - id: P1
    story: "nx ledger lint reads every doc: its frontmatter, its sources and its links, and a dead link fails the gate"
    state: done
    gate: "nx ledger lint checks docs/** and .rs doc refs; a test_reject_* per doc rule; just check green"
    started: 2026-11-02
    done: 2026-11-02
    evidence: "just check green; tools/nx/tests/ledger_docs.rs 20 tests (test_reject_* per doc rule); nx ledger lint 0 errors over 621 documents"
    note: "built: Kind::Doc, docs.rs/gen.rs/upkeep.rs, 19 tests; just check waits for every doc's frontmatter (P4)"
  - id: P2
    story: "one folder per subsystem, the dead status trackers and wish APIs gone, the research merged into its track"
    state: done
    gate: "words in scope <=60k; 0 dead doc links in docs/ and code"
    started: 2026-11-02
    done: 2026-11-02
    evidence: "48,926 words in scope (was ~105k); nx ledger lint: 0 dead doc links in docs/ and code; 0 notes name a moved or deleted doc"
  - id: P3
    story: "the pages that misled an agent say what the code does, and the tables a doc copies are generated from their source"
    state: done
    gate: "gen blocks syscalls, capabilities, markers fresh under nx ledger lint; 3 claims per rewritten page checked against the code"
    started: 2026-11-02
    done: 2026-11-02
    evidence: "gen blocks syscalls/capabilities/markers/docs-index fresh under nx ledger lint (a kernel comment edit was caught stale and re-rendered); claims checked per page against code by the rewrite pass"
  - id: P4
    story: "a code change under a doc's sources shows up in the brief, and the docs sweep has a command"
    state: done
    gate: "100% of docs in scope with frontmatter; nx ledger stale = 0 after the sweep; a probe edit under a source lists its doc"
    started: 2026-11-02
    done: 2026-11-02
    evidence: "57/57 docs with frontmatter; nx ledger stale = 0; probe: ledger_docs.rs stale_lists_a_doc_whose_source_changed_after_it_was_verified; just brief counts stale docs"
---

<!-- `nx ledger new task` copies everything below this line; the frontmatter above shows the
     planning fields (RFC-0115). Write state with `nx ledger`, never by hand. -->

## The ask

Who asked, in their words, and the outcome they want.

## Measured before the cut

What the code and the system do today — numbers, log paths, the measurement folder. A plan
that names no measurement is a guess.

## Three lenses

- **Scope** — in: … · out: … (with where each "out" goes).
- **Invariant** — what must hold, and the `test_reject_*` that proves it.
- **Contract** — the RFC/ADR this implements or seeds.

## Not doing here

The scope firewall: what is deliberately left out, and which task or milestone step takes it.

## Definition of Done

- Every phase `done` with its evidence; the goal's target reached on the instrument.
- Host tests, the QEMU lanes and the board cycle the phases name are green.
- Security-relevant work has its `test_reject_*` and hardening markers
  (`docs/standards/SECURITY_STANDARDS.md`).
- Docs swept: CHANGELOG, affected docs, the RFC's status; `active_days` and, when the estimate
  missed, a `variance_note`.
