# Open Nexus OS — Agent Guide (SSOT)

Single source of truth for AI agents working in this repo. Other entry points
(`AGENTS.md`) point here. Keep this file lean: rules and commands live here,
narratives live in `docs/`, execution state lives in `tasks/`.

## What this is

A research microkernel OS in Rust targeting RISC-V (`qemu-system-riscv64 virt`),
following seL4/Fuchsia principles: minimal kernel (sched/vm/ipc/caps), userspace
services, capability-based security, deny-by-default policy. Host-first
development, QEMU-last proof.

## Repo map

- `source/kernel/neuron/` — microkernel (no_std, RISC-V)
- `source/services/` — userspace daemons (windowd, gpud, samgrd, policyd, …)
- `source/drivers/` — userspace drivers (hardware-facing, contract-sensitive)
- `source/libs/` — shared libraries (nexus-abi, nexus-ipc, nexus-gfx, …)
- `userspace/` — host-compatible libraries and domain logic
- `tools/nx/` — DSL compiler/CLI + integration chain tests
- `tasks/` — task ledgers (execution truth) · `docs/rfcs/` — design contracts
- `docs/adr/` — architecture decisions · `docs/architecture/` — deep dives

## Build & verify

```bash
just check        # fast pre-commit gate: fmt + clippy + deny + arch (~3-5 min)
just test-host    # host test suite (workspace, kernel excluded)
just test-all     # full gate: check + tests + miri + kernel + QEMU SMP
just test-os      # QEMU selftest ladder (default profile=headless)
just start        # build + launch interactive OS (visible virgl window)
just dep-gate     # RFC-0009: forbidden-crate scan of the OS graph
just help         # full task catalog
```

- QEMU/build logs land in `build/logs/<profile>--<timestamp>/`
  (`uart.log`, `hypothesis.json`; decode via `docs/testing/run-logs.md`;
  `build/logs/latest` symlink; prune with `just logs-gc`).
- **One toolchain, pinned.** `rust-toolchain.toml` pins
  `nightly-2025-01-15` and EVERYTHING uses it — lint, fmt, host tests, the OS
  cross-build, `just start`, CI. Never introduce a floating `+stable` in a
  recipe: a stable/nightly split silently diverges (a lint on newer stable
  accepts APIs like `is_multiple_of` that the pinned build rejects; floating
  rustfmt reorders imports local≠CI). If clippy suggests an API newer than the
  pinned toolchain, keep the old form with `#[allow(unknown_lints, clippy::…)]`
  (the `unknown_lints` guard is required so the allow doesn't error on the
  pinned clippy). `just test-all` builds with this same toolchain, so a green
  `test-all` predicts a green `make build` / `just start` / CI.
- Formatting: **never plain `cargo fmt`** — config lives in
  `config/rustfmt.toml`. Approved command:
  ```bash
  cargo +nightly-2025-01-15 fmt --all -- --config-path config/rustfmt.toml
  ```
- **Warnings are a hard gate**, not telemetry: `just diag` (in `test-all`)
  denies any warning under host + os + kernel cfgs, and the real OS build
  (`scripts/build.sh` under `NEXUS_WARN_GATE=1`) fails on per-service warnings.
  Fix warnings or cfg-gate the code — do not add `#[allow(dead_code)]`; if an
  item is only used under one cfg, `#[cfg(nexus_env="os")]`-gate it so the
  other cfg never compiles it. (`NEXUS_ALLOW_WARN=1` is a local-only escape.)
- OS builds: services use `--no-default-features --features os-lite`;
  host/OS split via `nexus_env = "host" | "os"` cfg — never invent new cfgs.

## Hard rules

**Protection zones** (read freely; modifying needs explicit user approval):
`source/kernel/**`, `source/libs/**` (ABI!), root `Cargo.toml`, `Makefile`,
`scripts/`, `config/`, `recipes/meta/`, `docs/rfcs/` (RFC process applies).
Services under `source/services/**` are normal iteration ground; ABI-, security-
or windowing-relevant ones (windowd, samgr, policyd, dsoftbus, …) deserve extra
care and tests.

**Behavior-first proofs — no fake green.** UART markers are the proof of
behavior. `*: ready` / `SELFTEST: * ok` only after real behavior; stubs emit
`stub`/`placeholder`, never `ok`. Marker strings are stable, deterministic
contracts — changing one means updating `scripts/qemu-test.sh`, the marker
contract (`tools/nx/chains/markers.txt`), and docs in the same change.

**Security invariants** (see `docs/standards/SECURITY_STANDARDS.md`):
- Never log secrets/keys. Identity = `sender_service_id` from kernel IPC,
  never payload strings. Bound all input sizes before parsing.
- Sensitive ops route through `policyd` (single authority, deny-by-default).
- MMIO maps USER|RW only, never executable.
- Security-relevant changes need `test_reject_*` negative tests + hardening
  markers.
- No `unwrap`/`expect` on untrusted input; daemons propagate errors with
  context.

**Dependency hygiene** (RFC-0009): forbidden in the OS graph:
`parking_lot`, `parking_lot_core`, `getrandom`. Run `just dep-gate` before
committing OS-relevant changes. Licenses: Apache-2.0/MIT/BSD only
(`config/deny.toml`).

**Architecture boundaries** (crossing needs an ADR): kernel ↔ userspace
syscall ABI, service ↔ service IPC contracts, host ↔ OS feature gates,
policy authority. Keep drivers/policy out of the kernel. No Linux/Wayland
stacks — the UI path is windowd (compositor service) + app-host widgets.
windowd is a compositor SERVICE; window UI belongs in widgets, not windowd.

**Code style**: CONTEXT headers per `docs/standards/DOCUMENTATION_STANDARDS.md`
stay in sync; `#![forbid(unsafe_code)]` in userspace crates; no blanket
`#[allow(dead_code)]`; only core libraries carry the `nexus-` prefix; comments
in English. Prefer modular files (~600 LOC) over monoliths.

**Git**: never commit without explicit user approval in the current session —
propose a scoped commit message instead. Small commits, one intent each.
Work on feature branches, not `main`.

## Workflow

1. Read the task ledger (`tasks/TASK-*.md`) and its linked RFC/ADR contracts;
   stay inside the task's touched paths.
2. New syscall/service API/wire format/protocol → RFC seed first
   (`docs/rfcs/RFC-TEMPLATE.md`, next free number, update the RFC index).
3. Host-first: prove logic with host tests, then confirm in QEMU. Pick the
   smallest honest proof set; state target behavior → proof → regression
   signal.
4. Bounded debugging: form hypotheses, check `docs/testing/README.md`
   troubleshooting first, no endless retry chains.
5. Docs sweep before "done": task ledger status, RFC status, CHANGELOG,
   affected `docs/**`.

## Read more

- `docs/README.md` — documentation index
- `docs/architecture/README.md` — architecture entry point + reading order
- `docs/testing/README.md` — test layers, QEMU profiles, marker ladder
- `docs/standards/` — Rust/build/security/documentation standards
- `.claude/skills/` — boot-proof and verify workflows for this repo
