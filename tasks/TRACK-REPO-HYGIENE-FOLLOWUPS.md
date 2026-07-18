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

## 6. inputd chain I3/I4 markers on the live path

`inputd: chain I3 wire recv from hidrawd` / `chain I4 normalized` are only
emitted by the legacy recv loop (`os_lite.rs` trace_line sites) that the 111Hz
push path superseded — no real boot emits them anymore (interactive boots show
I1/I2 from hidrawd and I5/I6 delivery, but not I3/I4). The chain simulation
still models them. Re-emit both on the live path (logging-only change), then
move them back into the `input-live` group of `tools/nx/chains/markers.txt`.

## 7. cargo-deny host-only advisory ignores (2026 batch)

`config/deny.toml` ignores six 2026 RUSTSEC advisories, all verified host-only
(not in the riscv OS graph): ttf-parser/rustybuzz/memmap2 (unmaintained, host
font rendering — successor is skrifa/fontations), crossbeam-epoch/anyhow
(narrow host dev/test paths), quinn-proto (host QUIC test stack; RUSTSEC-2026-
0185 is a real remote-memory-exhaustion vuln but the OS transport does not link
it). Revisit: migrate host font shaping off ttf-parser/rustybuzz, and drop the
quinn ignore once the host QUIC dep bumps past the fix. The OS graph stays
guarded independently by `just dep-gate` (RFC-0009).
