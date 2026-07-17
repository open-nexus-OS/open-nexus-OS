# ADR-XXXX: <Title — one decision, stated as a sentence>

- Status: Proposed / Accepted / Superseded by ADR-YYYY / Deprecated
- Date: YYYY-MM-DD
- Links:
  - Tasks: `tasks/TASK-XXXX-...md` (execution + proof)
  - RFCs: `docs/rfcs/RFC-XXXX-...md` (the contract this decision feeds or narrows)
  - Related ADRs: `docs/adr/YYYY-...md`

## Context

What problem or fork-in-the-road exists? Why must a decision be made now?
Keep it to the facts that make the decision necessary — constraints, forces,
prior art in this repo. An ADR records **one decision, one rationale**; if you
are designing a contract with phases and proof gates, write an RFC instead
(`docs/rfcs/RFC-TEMPLATE.md`).

## Decision

The decision, stated normatively ("We will ...", "X owns Y", "Z is never ...").

- <the decision itself, in one or two sentences>
- <hard boundaries / invariants this decision fixes>
- <what is explicitly out of scope for this decision>

## Consequences

What becomes easier, what becomes harder, and what churn is accepted.

- **Positive**: <what this buys us>
- **Negative / accepted cost**: <churn, migrations, deprecations>
- **Follow-ups**: <tasks or RFCs that must exist for this to be real>

## Alternatives considered

- <alt 1> (why rejected)
- <alt 2> (why rejected)

---

## ADR Quality Guidelines (for authors)

- One decision per ADR; split unrelated decisions into separate ADRs.
- No fake certainty: if the decision is provisional, Status stays `Proposed`.
- If a later ADR/RFC replaces this one, update Status here
  (`Superseded by ...`) and add an index line note in `README.md`.
- Numbers are never reused; take the next free number (0019 is retired).
- Add your ADR to the index in `docs/adr/README.md`.
