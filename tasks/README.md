# Planning documents (RFC-0115)

Everything we plan and track lives in this folder as Markdown with canonical frontmatter. The
files are the single source of truth; `nx ledger` is the one writer of their state, the Section 9
ops room reads them, and every count or board is derived — never written by hand.

| Kind | Where | Job |
|---|---|---|
| Milestone | `milestones/<slug>.md` | a fast lane: goal, sections, steps in order, each step with its tasks |
| Sprint | `sprints/<YYYY-Www>.md` | one ISO week: capacity, the stories chosen (task + phases), the review |
| Task | `TASK-NNNN[X]-<slug>.md` | atomic work: goal with a number, consumer, phases as user stories with gates |
| Track | `TRACK-<NAME>.md` | vision; spawns tasks, never executed |
| Subsystems | `SUBSYSTEMS.md` | the subsystem slugs, their gate tier and release target |
| RFC | `../docs/rfcs/RFC-NNNN-<slug>.md` | a contract; progress lives in its tasks |

Milestones so far: `ui-fast-lane` (done), `sub-80` (done), `hardware-network` (active).

## Commands

```bash
just brief                              # the active milestone, this week's sprint, open work
nx ledger show TASK-0348                # one task: header, goal, one line per phase
nx ledger table                         # the counted progress table of the active milestone
nx ledger list --kind task --status in_progress
nx ledger phase TASK-0348 P7 --state building           # stamps started; `done` stamps done
nx ledger phase TASK-0349 P1 --story "…" --gate "…"     # a new phase (user story)
nx ledger goal TASK-0349 "<metric>" "<baseline>" "<target>" "<instrument>"
nx ledger step hardware-network L8.1 --state active --tasks TASK-0349
nx ledger new task --title "…" --subsystem ui --owner @ui
nx ledger new sprint --milestone hardware-network --capacity 5
nx ledger plan TASK-0349 P1,P2 --p50 2 --p85 3.5 --sample-size 6
just ledger-gate                        # the gate (part of `just check`); `nx ledger lint --fix` renders
```

## A task's life

1. **Planned** (`draft`): in a milestone step or the backlog. Before work starts it has a `goal`
   (metric, baseline, target, instrument), a `consumer` (who uses what it builds — named in the
   same milestone), an `estimate` from a reference class (subsystem, kind, novelty, with its
   sample), and phases. From 2026-11-03 on the gate refuses to start work without them.
2. **Building** (`in_progress`): the first phase that goes `building` starts the task. A phase is
   `building` → `proving` (built, waiting for its lane or board cycle) → `done` with its evidence
   path. Where the goal has a number, every phase gate names the value it must reach.
3. **Done**: every phase `done` or `dropped`, `active_days` recorded, a `variance_note` when the
   estimate missed — that note is what the next estimate learns from. Then the docs sweep:
   CHANGELOG, affected docs, the RFC's status.
4. **Superseded / deferred**: by decision, with the successor named (`superseded_by`).

The body of a ledger is narrative and decisions: the ask, what was measured before the cut, the
three lenses, what is not done here, the Definition of Done. It is not the state — the
frontmatter is. A long progress story is condensed when its phase closes; git keeps the history.

## The week

- **Monday, planning** in the Section 9 chat: the milestone, the open stories, the reference
  classes. The result is `sprints/<week>.md`, proposed as a diff and applied by the operator.
- **During the week**: the agent builds and writes phase states with `nx ledger`; the sprint
  board shows them live.
- **Monday, review**: completed and slipped stories, three lines of retrospective.

Planning and building run in parallel. **A task `in_progress` belongs to the building session**;
drafts, future tasks, milestones and sprints belong to the planning conversation.

## Rules that stay

- **One task at a time, 100 %.** Done means every stop condition met and its proof green.
- **No fake green.** A marker proves real behaviour; stubs say `stub`/`placeholder`. A marker is a
  contract: change it with `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt` and the manifest.
- **End system only.** A new mechanism replaces the old one in the same package, with a gate
  against its return; prerequisites are pulled into the lane, never bypassed.
- **When 100 % is out of reach:** split the task (suffix `B`, `C` for follow-ups), pull the
  minimal prerequisite forward (and note "extracted into TASK-…" in the later ledger), or narrow
  the stop conditions only if the milestone still means what it said.
- **Ledger reuse:** where a ledger exists for a subject, even an outdated one, it is recut to the
  end state; new numbers only where none exists.
- **Tracks are not tasks** and never hold hidden Definition of Done.
- **Visible proof (UI):** a UI capability is claimed only when it shows on the real desktop
  (QEMU and the board) and can be checked live.
- **Boundary:** windowd is the compositor service; window UI lives in widgets and the DSL shell;
  the GPU only through gpud/nexus-gfx; an ADR for any kernel↔userspace, service↔service,
  host↔OS or policy-authority crossing.

Template: `TASK-TEMPLATE.md`. Contract: `docs/rfcs/RFC-0115-…`.
