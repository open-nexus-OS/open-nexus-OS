# Contributing to Open Nexus OS

Thanks for your interest in contributing. Open Nexus OS is a research
microkernel OS for QEMU RISC-V `virt`, developed **host-first, QEMU-last**:
most logic lives in host-testable userspace crates, and QEMU boots are bounded
end-to-end proofs. This document explains how to get a working setup, how we
verify changes, and how decisions are made.

Please also read our [Code of Conduct](CODE_OF_CONDUCT.md) and
[Security Policy](SECURITY.md).

## Prerequisites and setup

```sh
git clone git@github.com:open-nexus-OS/open-nexus-OS.git
cd open-nexus-OS
make initial-setup
```

`make initial-setup` (see `scripts/install-deps.sh`) installs system packages
(capnproto, qemu-system-riscv64, mold, ...), the pinned Rust toolchains
(stable + `nightly-2025-01-15`, from `rust-toolchain.toml` and the justfile),
the `riscv64imac-unknown-none-elf` target, and required components.

Recommended: wire the pre-commit gate as a git hook so formatting/lint/license
drift is caught before it lands in a commit:

```sh
ln -sf ../../scripts/fmt-clippy-deny.sh .git/hooks/pre-commit
```

## Building and running

```sh
make build MODE=host   # compile everything (host workspace + cross OS services + kernel)
make run               # one bounded QEMU smoke against the build artifacts
just start             # build + launch the interactive OS (windowed QEMU)
```

There are two complementary entrypoints — `just` for granular host-first
iteration, `make` for the self-contained build → test → run pipeline. See
[README.md](README.md) for the full matrix.

## Verification ladder

The **justfile is the single source of truth for all gates.** CI
(`.github/workflows/ci.yml`) is a thin wrapper over the same recipes, so what
passes locally passes in CI.

| When | Command |
|---|---|
| Before **every** PR | `just check` (fmt + clippy + cargo-deny + arch-check, ~3-5 min) |
| Substantial changes | `just test-all` (full gate: check + deadcode + dep-gate + host/e2e tests + miri + kernel build + QEMU SMP lane, ~30 min) |
| OS-behavior changes | the relevant QEMU lane, e.g. `just test-os`, `just ci-os-smp`, `just ci-os-headless`, `just ci-os-full`, `just ci-network` |

Useful granular recipes: `just test-host`, `just test-e2e`, `just miri-strict`,
`just miri-fs`, `just deadcode`, `just dep-gate`, `just build-kernel`,
`just lint-kernel`. Run `just help` for the full catalog.

QEMU run logs land in `build/logs/<profile>--<timestamp>/` (uart.log,
qemu.stderr, hypothesis.json) — see `docs/testing/run-logs.md`. Prune with
`just logs-gc`.

## Authority model (how decisions are made)

- **Tasks (`tasks/TASK-*.md`) are the execution truth**: scope, stop
  conditions (DoD), and proof commands/markers. Start at `tasks/README.md`;
  day-to-day status is on `tasks/STATUS-BOARD.md`.
- **RFCs are design seeds / contracts**: new interfaces, invariants, or wire
  contracts need an RFC *before* implementation. Process:
  [docs/rfcs/README.md](docs/rfcs/README.md).
- **ADRs record architecture decisions**: one decision + rationale per file.
  Process: [docs/adr/README.md](docs/adr/README.md).

Practical rule: bug fixes and refactors within existing contracts just need a
task reference; anything that adds or changes a contract (syscalls, IPC
schemas, service boundaries, on-disk formats) needs an RFC first, and
significant architectural choices along the way get an ADR.

## Commits and pull requests

- **Small, scoped commits** — one intent per commit. Don't mix a refactor with
  a behavior change.
- **Conventional-commit-style subjects**, as seen in `git log`:
  `type(scope): summary` — e.g. `fix(gpud): scale response pool with
  RING_SLOTS`, `feat(smp): soft-realtime SMP=4 interactive default`,
  `docs(hygiene): restructure documentation tree`. Common types: `feat`,
  `fix`, `chore`, `docs`, `test`, `perf`, `style`.
- **No `Co-authored-by` trailers.**
- Update `CHANGELOG.md` (Unreleased section) when closing a task.
- PRs should state which task/RFC they implement and which gates were run.
  Keep PRs as small as the task allows; large tracks land as a sequence of
  reviewable slices.
- More detail: [docs/dev/git-workflow.md](docs/dev/git-workflow.md).

## Contributor License Agreement

First-time contributors are asked to sign the CLA; the `cla-check` workflow
prompts automatically on your first PR. See [.github/CLA.md](.github/CLA.md).

## Questions

Use [GitHub Discussions](https://github.com/orgs/open-nexus-OS/discussions)
for general questions. For security issues, follow [SECURITY.md](SECURITY.md)
— do not open public issues for exploitable findings.
