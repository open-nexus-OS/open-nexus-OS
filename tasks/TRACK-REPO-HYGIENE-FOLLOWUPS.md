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

## 8. Toolchain skew: `just lint` (+stable) vs OS build (nightly-2025-01-15)

`just lint` runs on floating `+stable` (currently 1.90); the OS/kernel build
uses the pinned `nightly-2025-01-15`. Newer clippy can suggest APIs the older
OS toolchain lacks — e.g. `unsigned_is_multiple_of` (stable 1.87) breaks the OS
build, so those sites keep `%` with `#[allow(clippy::manual_is_multiple_of)]`.
Consider pinning `just lint`/CI to a stable close to the OS nightly (or bumping
the OS nightly) so the lint surface matches what the OS build can compile.

## 9. Kernel clippy baseline (grandfathered style lints)

`source/kernel/neuron/src/lib.rs` grandfathers ~34 style/idiom clippy lint
categories (the kernel was never clippy-gated; the old ci-kernel.yml ran clippy
with `|| true`). `just lint-kernel` now gates the kernel with `-D warnings`, so
NEW lint types and all correctness lints stay hard errors, but the pre-existing
style findings are allowed rather than mass-rewritten in protected kernel code.
Shrink the allow list incrementally (each removal = fix the underlying sites +
verify build-kernel + a boot). The two genuine default-deny findings were fixed
outright (hex-literal regroup, dead self-assignment); `never_loop` and
`absurd_extreme_comparisons` are intentional idioms kept in the allow list.

## 10. SMP=2 (MTTCG) real-parallelism lane non-determinism

The `just ci-os-smp` lane runs SMP=2 under MTTCG (icount is impossible there —
it serializes harts and kills the cpu1/per-hart/IPI proofs). Beyond the now-fixed
UART interleaving (nexus-log `record_lock`, line-atomic), several of its timing
proofs are inherently non-deterministic under host-scheduling variance and flake
at ~2-3/5: `KSELFTEST: runtime timer budget ok`, `KSELFTEST: bkl budget ok`
values, and occasional service selftests (statefsd IPC `reply_and_close` fail
→ ladder stall, vfs/dsoftbus/apphost). Decision (2026-07): the hard test-all
boot gate is the DETERMINISTIC `ci-os-smp1` (SMP=1 + icount); the SMP=2 lane is
bounded-retry and CI-only. Follow-up: investigate the individual SMP=2 flakes
(statefsd reply-send robustness under concurrency; runtime-timer-budget margin
under MTTCG) so the parallelism lane can eventually gate without retry. Larger
kernel/scheduling scope, likely its own track.

## 11. Remaining `cfg_attr(not(os), allow(dead_code))` items (~120, os-live)

After Phase 2 (blanket allows removed, genuine dead code deleted, kernel clippy
baseline shrunk 36→7), the remaining dead_code allows in windowd/gpud/nexus-workpool
are `#[cfg_attr(not(all(feature="os-lite", nexus_env="os", target_os="none")),
allow(dead_code))]` on items that ARE used under the OS build and only look dead
under host. Verified: the real OS build (NEXUS_WARN_GATE=1 scripts/build.sh) is
warning-clean — these are not disabled errors, they are cross-cfg items suppressed
on the cfg where they aren't compiled into use. The strictly-cleaner form is to
`#[cfg(nexus_env="os")]`-gate each so host never compiles it (no allow attribute at
all), but that is a large mechanical pass over functionally-correct code. Optional
polish; do it if the visible `allow` attributes are undesirable, else leave — the
warning gate keeps the tree honest either way.
