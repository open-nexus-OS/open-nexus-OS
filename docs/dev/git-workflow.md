# Git Workflow

Honest description of how this repository is actually worked on today — not an
aspirational branching model.

## Branches

- **`main` is the integration branch.** Small, verified changes land on
  `main` directly; there is no `develop` branch.
- **Feature branches** (`feat/<topic>`, or `fix/<topic>` for larger bug-fix
  series) are used for larger work that spans multiple commits or needs
  review before integration. They merge back into `main` via PR.
- History is kept linear where practical — prefer rebasing a feature branch
  onto `main` over merge-commit chains.

## Commit style

Subjects follow a conventional-commit-style `type(scope): summary`, as
observed in `git log --oneline`:

```
fix(gpud): scale response pool + mappings with RING_SLOTS and raise ring to 32
feat(smp): soft-realtime SMP=4 interactive default
docs(hygiene): restructure documentation tree
chore(build): tooling SSOT — fmt-clippy-deny delegates to just
test(hygiene): remove stale ui_v3a_host/ui_v3b_host proof crates
```

- Common types: `feat`, `fix`, `chore`, `docs`, `test`, `perf`, `style`.
  Scope is optional but usual (a crate, service, or track name).
- **One intent per commit.** A refactor and a behavior change are two
  commits. A commit should reference the task/RFC it serves where relevant.
- No `Co-authored-by` trailers.

## Verify before you commit

The justfile is the gate SSOT; commit only what has passed the appropriate
rung of the ladder:

1. `just check` — always (fmt + clippy + cargo-deny + arch-check). Wire it as
   a pre-commit hook: `ln -sf ../../scripts/fmt-clippy-deny.sh
   .git/hooks/pre-commit`
2. `just test-host` / `just test-e2e` — when host-visible logic changed.
3. The relevant QEMU lane (`just test-os`, `just ci-os-smp`, ...) — when OS
   boot behavior changed.
4. `just test-all` — before closing a substantial task.

CI (`.github/workflows/ci.yml`) runs the same recipes, so a green local
ladder means a green PR.

## Worktrees

Git worktrees for parallel work live **outside** the repository checkout
(e.g. sibling directories). Agent-created worktrees under `.claude/worktrees/`
are gitignored; never commit a nested checkout.

## Changelog discipline

When a task is closed, add its summary to the **Unreleased** section of
`CHANGELOG.md` (Keep-a-Changelog format) in the same commit series that
closes the task.

## Related

- [CONTRIBUTING.md](../../CONTRIBUTING.md) — setup, verification ladder,
  authority model (tasks/RFCs/ADRs), PR conventions.
- `tasks/README.md` — task workflow and anti-drift rules.
