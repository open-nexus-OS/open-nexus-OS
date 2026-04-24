# Current Handoff: TASK-0046 in progress (config v1)

**Date**: 2026-04-24  
**Active execution task**: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` — `In Progress`  
**Contract seed**: `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md` — `In Progress`  
**Tier policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate J: DevX, Config & Tooling, `production-floor`)

## Baseline status

- `TASK-0045`/`RFC-0043` closure is complete and archived.
- Queue focus has moved to `TASK-0046` as execution SSOT for config v1.
- `RFC-0044` seed exists and is linked from task and RFC index.

## TASK-0046 target behavior

- Deterministic config layering and schema validation with bounded, fail-closed rejects.
- Canonical effective runtime/persistence snapshot is Cap'n Proto (JSON remains authoring/derived only).
- `configd` orchestrates 2PC reload with explicit commit/abort semantics.
- Config CLI surface stays under `nx config ...` (no `nx-*` drift).
- No marker-only closure evidence; no fake success.

## Initial gate matrix (Go / No-Go)

- **Gate A (canonical config authority floor)**: YELLOW
  - `TASK-0046` and `RFC-0044` are aligned; implementation surfaces not yet merged.
- **Gate B (format authority floor)**: YELLOW
  - Cap'n Proto-as-canonical contract is specified; schema + conformance proofs pending.
- **Gate C (proof quality floor)**: YELLOW
  - host proof matrix is defined; tests are not yet implemented/green.
- **Gate D (CLI/no-drift floor)**: YELLOW
  - `nx config` contract is specified; deterministic command proofs pending.
- **Gate E (2PC honesty floor)**: YELLOW
  - anti-fake-success requirement is explicit; state/result-correlated OS proof still pending.

## Planned proof floor (host-first)

- Primary proof commands:
  - `cd /home/jenning/open-nexus-OS && cargo test -p nexus-config -- --nocapture`
  - `cd /home/jenning/open-nexus-OS && cargo test -p configd -- --nocapture`
  - `cd /home/jenning/open-nexus-OS && cargo test -p nx -- --nocapture`
- Required reject/determinism focus:
  - layering precedence and bounded schema rejects,
  - canonical Cap'n Proto snapshot determinism,
  - 2PC abort path keeps previous effective version unchanged,
  - `nx config` deterministic exits/JSON contract.

## Guardrails

- Keep scope in TASK-0046 allowlist paths.
- Keep kernel untouched; host-first closure is authoritative for this cut.
- Preserve Cap'n Proto canonical authority and JSON derived-view discipline.
- Reject marker-only closure claims; require matching state/result assertions.
- Keep follow-up ownership explicit: `TASK-0047`, `TASK-0262`, `TASK-0266`, `TASK-0268`, `TASK-0273`, `TASK-0285`.

## Working set (.cursor sync checklist)

- `current_state.md`: active task/contract switched to `TASK-0046`/`RFC-0044`.
- `next_task_prep.md`: `TASK-0046` closure checklist and GO/NO-GO updated.
- `context_bundles.md`: `@task_0046_context` and `@task_0046_touched` added.
- `pre_flight.md`: Task-0046 automatic/manual addendum added.
- `stop_conditions.md`: Task-0046 class stop conditions added.
