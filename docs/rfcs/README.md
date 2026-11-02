# RFC process

An RFC is a **contract**: an interface, a wire format, an invariant, an architecture decision
that several tasks rely on. It is not a backlog and does not track progress.

1. Take the next free number: `nx ledger list --kind rfc | tail -1`.
2. Copy `RFC-TEMPLATE.md` to `RFC-NNNN-<short-title>.md`; fill the frontmatter (RFC-0115) and the
   contract. `just ledger-gate` checks the frontmatter.
3. Link the task(s) that build and prove it (`tasks:`); the tasks carry phases, proof and status.
4. When the contract changes, amend the RFC and name the task that proves the change.

**The index is derived**, never written by hand: `nx ledger list --kind rfc` (or
`--status in_progress`), and the RFCs view in Section 9.

## Authority model

- **Tasks (`tasks/TASK-*.md`) are the execution truth:** scope, phases, stop conditions, proof.
- **RFCs are contracts:** what the interface is, what must hold, what "stable" means. Each RFC is
  scoped so it can reach `done` once its tasks are proven; a follow-on that needs new behaviour
  gets a **new RFC** instead of growing an old one into a multi-phase backlog. A deferred
  capability is stated as out of scope, with the note that a new RFC will define it.
- **ADRs (`docs/adr/`) are narrow decisions:** one decision, one rationale.
- **Milestones and sprints (`tasks/milestones/`, `tasks/sprints/`)** are the order and the week.

**Contradictions:** on architecture or contract the RFC wins and the task is updated; on progress,
plan or proof signals the task wins and the RFC changes only if the contract did.

## Status

`draft` → `proposed` → `accepted` → `in_progress` (being built) → `done` (contract defined and its
proof gates green), or `superseded` with `superseded_by`. `status_note` is one line; history goes
to the body or to git.

## Security-relevant RFCs

An RFC touching crypto, authentication, identity, capabilities, untrusted input or sensitive data
carries a threat model, its security invariants, a DON'T-DO list and its proof strategy
(`test_reject_*`, hardening markers). See `docs/standards/SECURITY_STANDARDS.md`.
