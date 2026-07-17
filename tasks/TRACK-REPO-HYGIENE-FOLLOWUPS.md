# TRACK: Repo-Hygiene Follow-ups (2026-07-17)

Deferred items from the 2026-07 repository-hygiene track (structure/docs/test
hygiene only — no feature work). Each item is small and independent.

## 1. resources/ asset pruning (deferred by decision)

`resources/fonts/` (~39 MB, 6236 files) and `resources/icons/lucide/` (bulk of
~22 MB) are vendored upstream dumps; the build consumes only a small subset
(see `resources/README.md` — windowd/build.rs uses `InterVariable.ttf`, named
Lucide SVGs, mocu cursors, themes; gpud/build.rs uses one logo). Pruning needs:
usage analysis across `**/build.rs`, recipes and DSL manifests → delete unused
assets → boot proof (`just ci-os-display-gpu-pci` + visible `just start`).

## 2. ADR-0019 backfill decision

`docs/adr/` numbering has a gap at 0019 (never filed; documented as retired in
`docs/adr/README.md`). Decide once: keep retired forever (current stance) or
backfill with a real pending decision. Default: keep retired.

## 3. DSL doc-draft branch disposition

Branch `worktree-dsl-0075-frontend-ir-cli` archives an older snapshot of DSL
doc/task rewrites (verified 2026-07-17: main carries newer supersets of all 22
files). Safe to delete after a final skim; kept only as history.

## 4. cla-check.yml signature branch

`.github/workflows/cla-check.yml` stores signatures on branch
`neuron-foundation`; consider moving to a dedicated `cla-signatures` branch.

## 5. `make test` container ladder

`make test` still re-implements a QEMU ladder (headless + smp + dhcp) against
`scripts/qemu-test.sh` directly (kept self-contained by design — no `just`
dependency in the make spur). If the make spur ever grows, fold it onto the
`just ci-os-*` recipes instead.
