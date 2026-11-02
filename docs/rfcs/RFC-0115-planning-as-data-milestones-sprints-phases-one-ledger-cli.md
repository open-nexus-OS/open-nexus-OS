---
schema: 1
id: RFC-0115
title: "Planning as data: milestones, weekly sprints, phases as user stories, and one ledger CLI"
status: in_progress
status_note: "seeded 2026-11-02; format, CLI, gate and migration built in the same run"
owners: ["@tools-team"]
created: 2026-11-02
updated: 2026-11-02
subsystem: devx
tasks: [TASK-0349]
depends_on: []
links:
  - label: Reader
    target: "the Section 9 ops room (external tool, reads this format)"
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0115: Planning as data — milestones, weekly sprints, phases as user stories, and one ledger CLI

## Why

Tasks and RFCs were written so an agent understands the whole picture and a human can follow it.
With autonomous agents the human no longer reads the ledgers, and two failure modes showed up:

- **Nobody sees progress until the end of a session.** State lived in prose (`## Progress`
  sections, 3000-character cells in `IMPLEMENTATION-ORDER.md`), so the only overview was the
  table an agent wrote at the end.
- **Work without a number at its gate.** A GPU driver ran a week before a wrong flag was found,
  because its gate was "a picture appears", not "frame time below the stock system". A week of
  memory work went unused because no consumer was named (apps had hard-coded sizes). Cost
  objections came from textbook estimates instead of our own history.

The repository stays the single source of truth. This RFC makes the planning state machine-readable
so the agent, the human and an external reader (the Section 9 ops room) share it, and so history
can calibrate the next plan.

## Scope

- **Owns:** the document model under `tasks/` and `docs/rfcs/`, the canonical frontmatter (schema
  1), its rules, the `nx ledger` CLI as the one writer of planning state, the `ledger` gate, the
  weekly sprint rhythm and the ownership rule for parallel planning.
- **Does not own:** what a task builds (its ledger), architecture contracts (RFCs), decisions (ADRs).

## The document model

Each kind has one job. Anything derivable from other files is not written by hand.

| Kind | Path | Job |
|---|---|---|
| Milestone | `tasks/milestones/<slug>.md` | a fast lane: its goal, its sections and steps in order, each step with its tasks; decisions in the body |
| Sprint | `tasks/sprints/<YYYY-Www>.md` | one ISO week: capacity, the stories chosen (task + phases) with P50/P85 from history, the retrospective |
| Task | `tasks/TASK-NNNN[X]-<slug>.md` | atomic work: goal with a number, consumer, phases as user stories with gates; the body is narrative and decisions |
| Track | `tasks/TRACK-<NAME>.md` | vision; spawns tasks, never executed |
| RFC | `docs/rfcs/RFC-NNNN-<slug>.md` | a contract; no progress tracking beyond its status |
| Subsystems | `tasks/SUBSYSTEMS.md` | the subsystem slugs with gate tier and release target (the one list a task's `subsystem` refers to) |

`tasks/IMPLEMENTATION-ORDER.md` and `tasks/STATUS-BOARD.md` are deleted: the order is the
milestone files, the board and every counter are derived (`nx ledger list`, `nx ledger table`,
Section 9). The RFC index is derived (`nx ledger list --kind rfc`), never maintained by hand.

**Membership lives in one place.** A task belongs to a milestone only through a step's `tasks`
list; a task does not name its milestone. A task in no step is backlog.

## Canonical frontmatter (normative)

Every document starts with `---` frontmatter in a fixed YAML subset. `nx ledger fmt` defines the
rendering; the gate rejects anything that does not round-trip byte for byte.

- Top-level lines are `key: value`, `key: [a, b]`, or a block (`key:` followed by indented lines).
- Scalars are plain when they match `[A-Za-z0-9][A-Za-z0-9._/+-]*` and are not `true`, `false`,
  `null`, `yes`, `no`, `on`, `off`; otherwise double-quoted with JSON escapes.
- A block is either a map of scalars (two spaces) or a list of flat maps (`  - key: value`, then
  four spaces). Flat maps may hold scalars and flow lists only. Nothing nests deeper.
- Keys appear in the canonical order of their kind; unknown keys are rejected.
- Dates are `YYYY-MM-DD`. Owners are `@handle`. Ids are `TASK-NNNN[A-Z]`, `RFC-NNNN`, `TRACK-NAME`.

### Task

```yaml
schema: 1
id: TASK-0348
title: "Zero-copy surfaces, a compositor without fixed tables, and its restart"
status: in_progress            # draft | in_progress | done | superseded | deferred
status_note: "≤ 160 characters"
owner: ["@ui", "@runtime"]
created: 2026-10-31
updated: 2026-11-02
subsystem: ui                  # a slug from tasks/SUBSYSTEMS.md
components: [windowd, gpud]
kind: service                  # driver | protocol | service | ui | refactor | bring-up | hardening | contract | tooling | docs | other
novelty: first                 # first | variant | repeat
estimate: 4                    # active working days, half-day steps
started: 2026-10-31
done: 2026-11-04
active_days: 4.5
dates_source: recorded         # recorded | status-text | git-reconstructed
variance_note: "why the actual differs from the estimate"
goal:
  metric: "windowd copy per pass"
  baseline: "138 ms (board cycle 79)"
  target: "0"
  instrument: "windowd: compose copies="
  result: "0 (board cycle 83)"   # the measured value at close (rule 10)
consumer: hardware-network/L8.2  # a task, a phase (TASK-NNNN/P3) or a milestone step — never prose (rule 9)
depends_on: [TASK-0347]
follow_ups: [TASK-0349]
split_from: TASK-0348
superseded_by: [TASK-0350]
implements: [RFC-0114]
tracks: [TRACK-NEXUSGFX-SDK]
links:
  - label: "ADR"
    target: "docs/adr/0092-…"
phases:
  - id: P3
    story: "windowd composes app surfaces by reference, so a window costs no copy"
    state: done                # planned | building | proving | done | blocked | dropped
    gate: "windowd: compose copies=0 on headless + visible"
    metric: "copy per pass 138 ms -> 0"
    started: 2026-10-31
    done: 2026-11-01
    evidence: "build/logs/many-windows--2026-11-01T12-03-10"
    note: "free text, short"
```

A **phase is a user story**: one sentence of behaviour a user or a service sees, its gate (the
stop condition with its proof), and when it was built. `proving` means built and waiting for its
lane or board cycle — the column a reviewer watches.

### RFC

`schema, id, title, status (draft | proposed | accepted | in_progress | done | superseded),
status_note, owners, created, updated, subsystem, tasks, depends_on, extends, amends,
superseded_by, links`. Progress tables ("Status at a Glance", "Implementation Checklist") are no
longer part of the template: phases and proof live in the task.

### Track

`schema, id, title, status (draft | living | active | done | superseded), status_note, owner,
created, updated, links`.

### Milestone

```yaml
schema: 1
id: hardware-network
title: "Hardware and network fast track"
status: active                 # planned | active | done
started: 2026-09-21
goal: "the OS runs on the reference board: picture, USB input, memory, GPU, SMP, network, web"
sections:
  - key: B0
    title: "Tooling"
steps:
  - key: T0
    section: B0
    story: "one command installs the board tools on Ubuntu, Fedora and Arch"
    state: done                # planned | active | done | dropped
    done: 2026-09-21
    tasks: [TASK-0327]
    gate: "make doctor green on all three families"
```

### Sprint

```yaml
schema: 1
id: 2026-W45
milestone: hardware-network
capacity_days: 5
goal: "one sentence"
planned:
  - task: TASK-0348
    phases: [P6, P7]
    p50: 2
    p85: 3.5
    sample_size: 6
completed: [TASK-0348/P6]
slipped: [TASK-0348/P7]
```

### Subsystems

`schema` and `subsystems`, a list of `slug, name, tier (production-grade | production-floor |
beta-floor), release, focus`.

### Doc (TASK-0349)

Every page under `docs/` outside `rfcs/`, `adr/`, `board/`, `dev/`, `assets/`; its id is its path
without `.md` (`docs/storage/statefs`). Keys: `schema`, `title`, `role` (`index | guide |
reference | spec`), `subsystem`, `sources` (repo paths the page describes), `contracts`
(`RFC-NNNN`/`ADR-NNNN`), `verified` (the day it was last checked against its sources). The gate:
`sources` and `contracts` exist; every backticked repo path and markdown link in the prose
resolves; no task status words (status lives in the ledger); the prose outside generated blocks
stays within the role's word budget (index 800, guide 1200, reference 2000, spec 4000); every
generated block (`<!-- gen:syscalls|capabilities|markers|docs-index -->`) equals its source's
rendering (`lint --fix` renders it). Beyond the documents: a `docs/…md` named from a code comment,
a root file or `.claude/` exists (fatal); one named from a planning document is a note, fatal for a
task started under the new process.

## Rules the gate enforces

1. Round trip: parse → render equals the file; keys known and in order; id matches the file name.
2. Status enums; `status_note` ≤ 160 characters (long history goes to the body, never the header).
3. `in_progress` → `started`; `done` → `done` and every phase `done` or `dropped`;
   `superseded` → `superseded_by`; any of `started`/`done` → `dates_source`; `done` ≥ `started`.
4. Phases: unique ids (`P` + number + optional letter), a gate, `building`/`proving` → `started`,
   `done` → `done`.
5. **From the cutover (tasks started on or after 2026-11-03)** an `in_progress` or `done` task
   carries `goal`, `consumer`, `estimate`, `kind`, `novelty` and at least one phase; a `done`
   phase carries `evidence`; a `done` task carries `active_days` and, when it had an estimate, a
   `variance_note`. Earlier tasks are grandfathered — their dates are reconstructed and kept apart.
6. Milestones: unique step keys, every step names a known section, a `done` milestone has only
   `done`/`dropped` steps.
7. Referenced ids that do not exist are reported, not fatal (history mentions retired ledgers).
8. **An open phase while one waits.** An `in_progress` task with a `planned` phase has a phase in
   `building`, `proving` or `blocked`; the board always shows where the work is. Every phase
   finished is the closing, not a gap.
9. **The consumer is a reference**: a task, a phase (`TASK-NNNN/P3`) or a milestone step
   (`<milestone>/<KEY>`), never prose and never the task itself.
10. **`done` closes the goal with its result**: a done task with a goal carries `goal.result`,
    the measured value against the target.
11. **Done is done for somebody**: at `done` the consumer exists and has at least started
    (task `in_progress`/`done`, phase open or done, step `active`/`done`). The memory week could
    not have closed with the apps' sizes still hardcoded.
12. **A milestone step agrees with its tasks**: no `done` step with a task still `draft` or
    `in_progress`, no `planned` step with a task `in_progress`. Starting a task's first phase
    activates its planned steps (derived, not typed).

Rules 8–12 are fatal for tasks started on or after the cutover and notes for the grandfathered
ones (`nx ledger lint --notes` lists them for the Monday review). Every `nx ledger` write runs the
cross-document checks too, so a refused edit is refused before it lands.

## Rules for the work itself

- **A number at every gate.** Where the goal has a metric, each phase's gate names the value it
  must reach (the GPU flag would have failed P2, not a week later).
- **A named consumer.** A mechanism without a consumer in the same milestone is not planned; the
  consumer becomes part of the package (the memory week would have been planned with the apps'
  dynamic sizes).
- **Estimates from history, never from the textbook.** A plan names its reference class
  (subsystem, kind, novelty) and the sample it rests on. A cost objection without such a class
  is not an objection. The closing `variance_note` is what the next estimate learns from.

## The CLI (`nx ledger`)

The one writer of planning state for agents and humans; the external reader proposes edits in
the same format.

```text
nx ledger brief                       # session start: active milestone, this week's sprint, open work
nx ledger show <ID>...                # compact view: header line, goal, one line per phase
nx ledger list [--kind] [--status] [--subsystem]
nx ledger table [<milestone>]         # progress table for a report, counted, never estimated
nx ledger set <ID> <key> <value>      # status changes stamp started/done with today
nx ledger goal <ID> <metric> <baseline> <target> <instrument>
nx ledger result <ID> <value>         # goal.result: the measured value at close
nx ledger phase <ID> <P> [--state] [--story] [--gate] [--metric] [--evidence] [--note]
nx ledger step <milestone> <KEY> [--state] [--story] [--section] [--tasks] [--gate]
nx ledger plan <ID> <P,P> [--week] [--p50] [--p85] [--sample-size]
nx ledger review [<week>]             # completed/slipped counted from the phases, written, printed
nx ledger new task --title … --subsystem … --owner …   # next free number, template body
nx ledger new sprint --milestone … --capacity …
nx ledger lint [--fix] [--notes]      # the gate; --fix renders canonically (and generated blocks)
nx ledger stale                       # docs whose sources changed in a commit after `verified`
nx ledger relink <old> <new|->        # a doc moved or merged: rewrite every reference; `-` drops links
```

The CLI edits only the frontmatter; the body is preserved byte for byte (exceptions: `relink`
rewrites paths in bodies, `lint --fix` renders a doc's generated blocks). Output is compact by
design: an agent reads `show` (a few hundred tokens) instead of a 20 KB ledger.

## The weekly rhythm and parallel planning

- **Monday, planning (Section 9 chat).** Milestone, open stories, reference classes; the result is
  `tasks/sprints/<week>.md`, proposed as a diff and applied by the human.
- **During the week, building (agent).** Phase states via `nx ledger phase`; the reader shows the
  sprint board live from the working copy.
- **Monday, review.** `nx ledger review` (or Section 9's `get_sprint` → `outcome`) counts
  `completed`/`slipped` from the phases and writes them; three lines of retrospective in the
  sprint body; the closed tasks' `variance_note`s become lessons per subsystem.

**Ownership rule.** A task `in_progress` belongs to the building session; drafts, future tasks,
milestones and sprints belong to the planning conversation. The reader proposes no frontmatter
change to an `in_progress` task. Applying a proposal fails if the file changed since it was made.

## Migration (2026-11-02)

- Every task, track and RFC rewritten to schema 1 by one script over the external reader's
  lenient parser; the body kept, a long status moved into the body as a `> Status (migrated
  2026-11-02): …` line. `created` from the header or the file's first commit; `started`, `done`
  and `active_days` of finished tasks from commit subjects naming the task (`dates_source:
  git-reconstructed`); `subsystem` from the status board's groups.
- The three milestones written from the history of `IMPLEMENTATION-ORDER.md`: `ui-fast-lane`
  (2026-04-21 → 2026-07-22), `sub-80` (2026-09-03 → 2026-09-21, its remainder ran as Block 3 of the
  next), `hardware-network` (active since 2026-09-21).
- Active tasks re-cut by hand with goal, consumer and phases.

## Proof

```bash
cargo test -p nx --test ledger_cli    # parser, renderer, rules, edits (test_reject_* for every rule)
just ledger-gate                      # every document lints; part of `just check`
# Section 9: tests/core/data-repo.test.ts parses and re-renders every document of this repo
# byte for byte through its own schema and renderer — the two implementations cannot drift.
```

## Alternatives considered

- **A database in the external tool as the truth:** rejected; two truths drift, and an agent in
  this repo must plan from files alone.
- **Full YAML with a parser crate:** rejected; the format is ours, a subset keeps round trips exact
  and edits format-preserving without a new dependency in the workspace.
- **Keeping `IMPLEMENTATION-ORDER.md` as a view:** rejected; it was the main place prose drifted,
  and a view that is derived is not written.
