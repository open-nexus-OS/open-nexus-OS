# Open Nexus OS — Agent Guide (SSOT)

Single source of truth for AI agents working in this repo. Other entry points
(`AGENTS.md`) point here. Keep this file lean: rules and commands live here,
narratives live in `docs/`, execution state lives in `tasks/`.

## What this is

An independent capability-microkernel OS in Rust for RISC-V (`qemu-system-riscv64 virt`
and a real RISC-V board),
following capability-microkernel principles: minimal kernel (sched/vm/ipc/caps), userspace
services, capability-based security, deny-by-default policy. Host-first
development, QEMU-last proof.

## Repo map

- `source/kernel/neuron/` — microkernel (no_std, RISC-V)
- `source/services/` — userspace daemons (windowd, gpud, samgrd, policyd, …)
- `source/drivers/` — userspace drivers (hardware-facing, contract-sensitive)
- `source/libs/` — shared libraries (nexus-abi, nexus-ipc, nexus-gfx, …)
- `userspace/` — host-compatible libraries and domain logic
- `tools/nx/` — DSL compiler/CLI + integration chain tests
- `tasks/` — planning as data (RFC-0115): `milestones/` (the order), `sprints/` (the week),
  `TASK-*` ledgers (execution truth), `SUBSYSTEMS.md` · `docs/rfcs/` — design contracts
- `docs/adr/` — architecture decisions · `docs/<subsystem>/` — how the code fits together
  (one folder per subsystem; map in `docs/README.md`)

## Build & verify

```bash
just check        # fast pre-commit gate: fmt + clippy + deny + arch (~3-5 min)
just test-host    # host test suite (workspace, kernel excluded)
just test-all     # full gate: check + tests + miri + kernel + QEMU SMP
just test-os      # QEMU selftest ladder (default profile=headless)
just start        # build + launch interactive OS (visible virgl window)
just dep-gate     # RFC-0009: forbidden-crate scan of the OS graph
just brief        # RFC-0115: active milestone, this week's sprint, open work (also at session start)
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

Canonical rule = inside the tag; prose = the "why". Tag vocabulary is tiny on
purpose (`protection_zones`, `critical`, `forbidden`, `verification_gate`) so
`critical` stays scarce.

<rules>

<protection_zones> <!-- read freely; access="approval" ⇒ modifying needs explicit user approval; enforced by .claude/hooks/protect-zones.py (PreToolUse → prompts for approval) -->
<zone path="source/kernel/**" access="approval" reason="microkernel"/>
<zone path="source/libs/**" access="approval" reason="ABI stability"/>
<zone path="Cargo.toml" access="approval" scope="root"/>
<zone path="Makefile" access="approval"/>
<zone path="scripts/**" access="approval"/>
<zone path="config/**" access="approval"/>
<zone path="recipes/meta/**" access="approval"/>
<zone path="tasks/**" access="approval" reason="execution truth" note="frontmatter (status, title, phases, goal, steps) ONLY via `nx ledger` — no prompt; a body edit by hand prompts; never sed/python into tasks/"/>
<zone path="source/services/**" access="iteration" note="iteration ground; windowd/samgr/policyd/dsoftbus (ABI/security/windowing) need extra care + tests"/>
</protection_zones>

<critical id="no-fake-green"> <!-- gate: scripts/check-chain-markers.sh -->
UART markers = proof of behavior. `*: ready` / `SELFTEST: * ok` ONLY after real
behavior. Stubs → `stub`/`placeholder`, never `ok`. Marker = stable contract:
change one → update `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` + docs
together.
</critical>

<critical id="no-auto-commit">
NO commit without explicit user approval this session. Propose a scoped message
instead. Small commits, one intent. Feature branches, never `main`.
</critical>

<critical id="security-invariants" ref="docs/standards/SECURITY_STANDARDS.md">
Never log secrets/keys. Identity = `sender_service_id` from kernel IPC, never
payload strings. Bound input sizes before parsing. Sensitive ops → `policyd`
(deny-by-default). MMIO = USER|RW only, never exec. No `unwrap`/`expect` on
untrusted input. Security changes need `test_reject_*` tests + hardening markers.
</critical>

<forbidden>
<item>Blanket `#[allow(dead_code)]` — cfg-gate under `nexus_env="os"` instead</item>
<item>`unwrap`/`expect` on untrusted input · logging secrets/keys</item>
<item>`parking_lot`, `parking_lot_core`, `getrandom` in the OS graph (RFC-0009)</item>
<item>Floating `+stable` in any recipe (one pinned nightly) · new cfgs beyond `nexus_env="host"|"os"`</item>
</forbidden>

<verification_gate name="warnings" enforced-by="just diag ; scripts/build.sh NEXUS_WARN_GATE=1">
Zero warnings under host+os+kernel cfgs is a HARD FAILURE. Fix or cfg-gate — never blanket-allow.
</verification_gate>

<verification_gate name="dependency-hygiene" enforced-by="just dep-gate ; config/deny.toml">
Forbidden crates (see &lt;forbidden&gt;) must not enter the OS graph. Licenses: Apache-2.0/MIT/BSD only.
</verification_gate>

<verification_gate name="structure" enforced-by="just structure-gate (in just check)">
Module-size ratchet: no .rs file > ~600 LOC unless grandfathered in config/loc-baseline.txt
(may shrink, never grow). New OS services ship src/ + tests/ (config/service-layout.allow).
</verification_gate>

<verification_gate name="architecture-boundaries" enforced-by="just arch-check ; arch-gate ; ADR required">
Crossing kernel↔userspace ABI, service↔service IPC, host↔OS gates, or policy authority
needs an ADR. Keep drivers/policy out of the kernel. No Linux/Wayland — the UI path is
windowd (compositor SERVICE) + app-host widgets; window UI belongs in widgets, not windowd.
</verification_gate>

</rules>

**Conventions** (style, not damage-on-lapse invariants): CONTEXT headers per
`docs/standards/DOCUMENTATION_STANDARDS.md` stay in sync; `#![forbid(unsafe_code)]`
in userspace crates; only core libraries carry the `nexus-` prefix; comments in
English; prefer modular files (~600 LOC) over monoliths.

## Workflow

1. Orient with `just brief` / `nx ledger show <ID>` (compact; read a ledger body only for the
   decisions you need) and the linked RFC/ADR contracts; stay inside the task's touched paths.
   **Planning state is written only through `nx ledger`** (phase, set, step, goal, plan) — never
   by editing frontmatter or prose progress by hand; `just ledger-gate` (in `just check`)
   enforces the format. Before a task starts: the `plan-package` skill (goal with a number,
   named consumer, estimate from history, phases as user stories with measurable gates).
   A task `in_progress` belongs to the building session; drafts, milestones and sprints may
   change in parallel from the planning conversation (Section 9).
2. New syscall/service API/wire format/protocol → RFC seed first
   (`docs/rfcs/RFC-TEMPLATE.md`, next free number; the index is derived: `nx ledger list --kind rfc`).
3. Host-first: prove logic with host tests, then confirm in QEMU. Pick the
   smallest honest proof set; state target behavior → proof → regression
   signal.
4. Bounded debugging: form hypotheses, check `docs/testing/run-logs.md` (hypothesis IDs)
   first, no endless retry chains.
5. Docs sweep before "done": phases and status via `nx ledger`, RFC status, CHANGELOG, and
   `nx ledger stale` — every doc whose `sources` you changed: fix it, then
   `nx ledger set docs/<path> verified <today>`; a moved doc: `nx ledger relink <old> <new>`.
   Docs carry no task status. Progress reports use `nx ledger table` (counted, never estimated).

## Read more

- `docs/README.md` — the docs entry point: one folder per subsystem, every page with its role
  (read a page's 5-line header first; open the body only if it answers your question)
- `.claude/skills/` — plan-package (RFC-0115 planning ritual) + code-quality + architecture-review
  (design gate) + boot-proof + verify workflows
- `.claude/skills/driver-bringup/` — driver / board / SoC-block bring-up playbook: measure on the
  stock system → hypotheses with a decision criterion → gate → code → gate → sharpen (recipes,
  gate library, traps, ledger template)
