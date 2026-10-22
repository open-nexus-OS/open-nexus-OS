# Contributing to Open Nexus OS

Thanks for your interest in contributing. Open Nexus OS is an independent,
capability-based operating system for RISC-V, from microkernel to desktop. It
runs on QEMU RISC-V `virt` and on a real RISC-V single-board computer, and it
is developed **host-first, QEMU-last**: most logic lives in host-testable
userspace crates, and QEMU boots are bounded end-to-end proofs. This document explains how to get a working setup, how we
verify changes, and how decisions are made.

Please also read our [Code of Conduct](CODE_OF_CONDUCT.md) and
[Security Policy](SECURITY.md).

## Prerequisites and setup

Supported hosts: Debian/Ubuntu, Fedora, Arch. The checkout **must live under
`$HOME`** (rootless podman + cargo caches need user-owned paths); setup refuses
to run anywhere else.

```sh
git clone git@github.com:open-nexus-OS/open-nexus-OS.git
cd ~/open-nexus-OS
make initial-setup
```

Prerequisites are `git`, `curl` and `make` — a bare distro install has no `make`
(it arrives with `build-essential` / `base-devel`), and no Makefile can bootstrap
the tool that reads it. `./scripts/install-deps.sh` does the same package step
with nothing but bash if you prefer; `make initial-setup` is idempotent either
way.

`make initial-setup` (see `scripts/install-deps.sh`) installs the system
packages (a C toolchain, capnproto, ripgrep, just, QEMU for RISC-V incl. its
GTK/OpenGL display backends, podman, ...), the pinned Rust toolchains
(stable + `nightly-2025-01-15`, from `rust-toolchain.toml` and the justfile),
the `riscv64imac-unknown-none-elf` target, the required components (including
`miri`, which `just test-all` needs), `cargo-deny` and `cargo-nextest`; fetches
the declared build inputs (`scripts/fetch-inputs.sh`: three submodules + pinned
Noto fonts); and wires `scripts/fmt-clippy-deny.sh` as your `pre-commit` hook —
it leaves an existing hook untouched, link it yourself with
`ln -sf ../../scripts/fmt-clippy-deny.sh .git/hooks/pre-commit`.

**Run it as yourself, not with `sudo`.** The setup needs root for exactly three
things — installing packages, the board's udev rule, your serial-group
membership — and the scripts ask for your password at those points. Under
`sudo make initial-setup` everything else (rustup in `~/.cargo`, cargo's
`target/`, rootless podman) would be created for root instead of you, so
`make doctor` refuses to continue and says why. The prompt needs a real
terminal: an editor/agent shell without a TTY fails with "sudo needs a password
but this shell has no terminal" — run `sudo -v` in a terminal first, or run
the whole command there.

The setup also installs the board tools (`fastboot`, a serial terminal,
`mkimage`, `dtc`, `sgdisk`) plus the udev rule and serial-group membership that
let `just board-*` reach the reference board without sudo.

| Flag | Effect |
| --- | --- |
| `YES=1` | non-interactive (no sudo prompt) |
| `GUI=0` | skip GTK/EGL/virgl — headless lanes only, `just start` will not work |
| `PODMAN=0` | skip the rootless-podman checks; use `make build MODE=host` |
| `BOARD=0` | skip the board flash/serial tools + udev rule — no reference board on this host |

`just start` opens a real virgl window (`-display gtk,gl=on`). On Debian/Ubuntu
the GTK and OpenGL display backends live in packages separate from the RISC-V
emulator (`qemu-system-gui`, `qemu-system-modules-opengl`); `make initial-setup`
installs them unless you pass `GUI=0`.

```sh
make doctor    # verify a host can build/test/run — no sudo, seconds
```

`make doctor` checks capabilities rather than package names (binaries, Python
≥ 3.11, toolchain/target/components, `llvm-objcopy`, QEMU's `virt` machine and
`gtk`/`egl-headless` display backends, submodules + fonts, rootless podman) and
prints the exact fix for each failure. Package *names* differ per distro and per
release; capabilities do not — run this whenever a build behaves oddly on a new
box.

## Building and running

```sh
make build MODE=host   # compile everything (host workspace + cross OS services + kernel)
make run               # one bounded QEMU smoke against the build artifacts
just start             # build + launch the interactive OS (windowed QEMU)
```

There are two complementary entrypoints, neither of which calls into the other:

- `just …` — developer ergonomics for host-first iteration and the granular
  QEMU profile matrix. Lazy-build: every recipe rebuilds what it needs.
- `make …` — the self-contained "build once, then test/run against the
  artifacts" pipeline. It owns its own build step and is not a shim over `just`.

| Target | Owns the build? | Runs QEMU? | Notes |
|---|---|---|---|
| `make build` | yes (single source of truth for compilation) | no | `MODE=container` (default, podman) or `MODE=host` (direct cargo). Builds **and** pre-compiles host test binaries. |
| `make test` | no — requires `make build` | yes — SMP=2 (`--profile=smp`) then SMP=1 (`--profile=full`) | Host tests via `cargo nextest`, then the QEMU ladder. |
| `make run` | no — requires `make build` | yes — single boot, profile from `SMP` env (or `PROFILE=…`) | `RUN_UNTIL_MARKER=1 make run` is the default fast loop. |

`make test` and `make run` set `NEXUS_SKIP_BUILD=1`, so a missing artifact is a
loud one-line `run 'make build' first` error instead of a silent rebuild. Use
`NEXUS_SKIP_BUILD=0 make run` or `make build run` for the eager-rebuild behavior.

### QEMU and persistence

StateFS persistence requires **modern virtio-mmio** semantics for virtio-blk.
The canonical harness (`scripts/qemu-launcher.sh`, driven by
`scripts/qemu-test.sh` / `just test-os`) sets
`-global virtio-mmio.force-legacy=off`. An external harness that cannot set
QEMU globals can build the force-modern QEMU variant:

```sh
./tools/qemu/build-modern.sh
export PATH="$PWD/tools/qemu-src/build:$PATH"
```

## Verification ladder

The **justfile is the single source of truth for all gates.** CI
(`.github/workflows/ci.yml`) is a thin wrapper over the same recipes, so what
passes locally passes in CI.

| When | Command |
|---|---|
| Before **every** PR | `just check` (fmt + clippy + cargo-deny + arch-check, ~3-5 min) |
| Substantial changes | `just test-all` (full gate: check + deadcode + dep-gate + host/e2e tests + miri + kernel build + QEMU SMP lane, ~30 min) |
| OS-behavior changes | the relevant QEMU lane, e.g. `just test-os`, `just ci-os-smp`, `just ci-os-headless`, `just ci-os-full`, `just ci-network` |
| Security-relevant changes | `cargo test -p nexus-abi -- reject --nocapture`, `just dep-gate`, `just diag-os`, `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` |

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

Use [GitHub Discussions](https://github.com/open-nexus-OS/open-nexus-OS/discussions)
for general questions. For security issues, follow [SECURITY.md](SECURITY.md)
— do not open public issues for exploitable findings.
