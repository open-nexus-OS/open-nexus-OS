---
schema: 1
title: "Documentation standards"
role: reference
subsystem: devx
sources: [tools/nx/src/commands/ledger, justfile, ".github/CODEOWNERS"]
contracts: [RFC-0115]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Documentation standards

> **Purpose** — how source headers, ADRs and pages under `docs/` are written, and what the gate checks.
> **Owns** — the doc rules `nx ledger lint` enforces (`tools/nx/src/commands/ledger/`)
> **Contracts** — [RFC-0115](../rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md) (planning as data, one ledger CLI)
> **Invariants** — status lives in the ledger, never in a doc; every path and link in a doc resolves; a doc describes what the code does now.
> **Read next** — [../README.md](../README.md), [RUST_STANDARDS.md](RUST_STANDARDS.md)

## Source file headers

Every Rust source file starts with the license lines and a CONTEXT header:

```rust
// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: [one line: what this module does and why]
//! OWNERS: @team-name
//! STATUS: Functional | Experimental | Placeholder | Deprecated
//! API_STABILITY: Stable | Unstable | Internal
//! TEST_COVERAGE: [tests that cover it, or "No tests"]
//! ADR: docs/adr/NNNN-slug.md
```

| field | meaning |
|---|---|
| `CONTEXT` | purpose; a split-out module says what it was split from |
| `OWNERS` | review responsibility (`@kernel-team`, `@runtime`, `@ui`, `@tools-team`, …) |
| `STATUS` | `Functional` implemented and tested · `Experimental` API may change · `Placeholder` stubs only · `Deprecated` will go |
| `API_STABILITY` | `Stable` for external consumers · `Unstable` · `Internal` |
| `TEST_COVERAGE` | the tests or markers that prove it (a count, a test file, a marker) |
| `ADR` | the ADR or RFC the module implements; it must exist |

Optional sections for complex modules: `PUBLIC API`, `DEPENDS_ON`, `INVARIANTS`,
`SECURITY INVARIANTS`, `ERROR CONDITIONS`. Kernel files use `PUBLIC API`, `DEPENDS_ON` and
`INVARIANTS`. Test files add `TEST_SCOPE` and `TEST_SCENARIOS`. Shell scripts use the same fields
as `#` comments; assembly files a `/* … */` block with responsibilities and notes.

Example:

```rust
//! CONTEXT: the clipboard history — bounded, newest first, fixed storage
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs
//! ADR: docs/adr/0070-clipboardd-one-authority-focus-gated-reads-ui-in-shell-and-keyboard.md
```

Update `STATUS`, `TEST_COVERAGE` and invariants when the implementation or its risk changes. No
gate checks header presence or fields; review does. The gate does check that every
`docs/<path>.md` a code comment names exists (see *The gate*).

## ADRs

`docs/adr/<NNNN>-<slug>.md`, next free number, the slug a short sentence of the decision. Sections:

```markdown
# ADR-NNNN: <the decision as a sentence>

## Status
Proposed | Accepted | Deprecated | Superseded by ADR-MMMM

## Context
## Decision
## Consequences
```

A new syscall, service API, wire format or protocol starts as an RFC seed
(`docs/rfcs/RFC-TEMPLATE.md`); the RFC index is `nx ledger list --kind rfc`.

## CODEOWNERS

`.github/CODEOWNERS` assigns everything to the maintainer (`* @jenningschaefer`). The team lines
below it are commented out until the GitHub teams exist.

## Docs under `docs/`

Every page outside `docs/rfcs/`, `docs/adr/`, `docs/board/`, `docs/dev/` and `docs/assets/` has
this shape:

```markdown
---
schema: 1
title: "Human title"
role: guide
subsystem: storage
sources: [userspace/statefs, source/services/statefsd]
contracts: [RFC-0018, ADR-0024]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Human title

> **Purpose** — one sentence: what this page answers.
> **Owns** — the code paths it explains
> **Contracts** — links to the RFCs and ADRs
> **Invariants** — the 1–3 rules an agent must not break
> **Read next** — the next pages
```

| key | rule |
|---|---|
| `role` | `index` (a folder map), `guide` (orientation for a subsystem), `reference` (lookup tables, CLI, operations), `spec` (a normative format with no RFC behind it) |
| `subsystem` | one of kernel, distributed, networking, observability, sessions, security, storage, updates, hardware, ui, text, media, messaging, app-platform, devx |
| `sources` | 1–6 code paths the page describes; required except for an index; each must exist |
| `contracts` | RFC-NNNN / ADR-NNNN ids that exist |
| `verified` | the day the page was last checked against its sources |

### Word budgets

Prose outside generated blocks: index ≤ 800 · guide ≤ 1200 · reference ≤ 2000 · spec ≤ 4000.
Over budget: cut or split. Prefer tables and lists over prose.

### Content rules

- Describe what the code does now. Verify every concrete claim; leave out what you cannot verify.
- No task status: no check-mark tables, review states, "as of" dates, last-updated stamps,
  status lines, phase logs or time estimates (the gate lists the exact words it rejects in
  `tools/nx/src/commands/ledger/docs.rs`). A task id may appear as a pointer to history,
  never with a status.
- Do not restate an RFC or ADR — link it and say in one line what it decides. A page is orientation
  plus what the RFC does not cover (operations, formats without an RFC, how pieces connect).
- Every backticked path starting with `source/`, `userspace/`, `tools/`, `scripts/`, `docs/`,
  `tasks/`, `recipes/`, `policies/`, `config/`, `schemas/`, `tests/`, `resources/`, `podman/`,
  `.claude/` or `.github/` must exist (a `:line` or `#anchor` suffix is fine); every markdown link
  must resolve. Placeholders use angle brackets: `source/services/<name>d/`. Fenced code is not
  checked.

### Generated blocks

A block between `<!-- gen:NAME -->` and `<!-- /gen:NAME -->` (each marker on its own line) is
rendered from the code by `nx ledger lint --fix`; never edit inside it.

| block | rendered from |
|---|---|
| `syscalls` | the kernel's syscall table |
| `capabilities` | capability → holders from `policies/base.toml` |
| `markers` | proof-manifest phases with marker counts |
| `docs-index` | every page with its role (the docs index) |

### Keeping pages true

| when | run |
|---|---|
| any time | `nx ledger stale` — pages whose `sources` changed in a commit after their `verified` day; `just brief` shows the count |
| after fixing a stale page | `nx ledger set docs/<path-without-.md> verified <YYYY-MM-DD>` |
| after moving or merging a page | `nx ledger relink <old> <new>` — rewrites every reference in planning documents and `docs/`, and lists what still names the old path |
| after deleting a page | `nx ledger relink <old> -` — drops the `links` records that pointed at it |

## Change management

When a change touches architecture, security boundaries, ABI, IPC, capabilities, the loader or the
testing workflow, the docs are part of done:

- the RFC that owns the contract, and the ADR when a decision moved;
- `docs/testing/` when markers, lanes or required negative tests change;
- the affected pages under `docs/` (then `verified`);
- source CONTEXT headers.

Logs and markers go through the shared facade (`nexus-log`, `log_*` macros) and stay deterministic
and honest: `*: ready` or `SELFTEST: … ok` only after the behavior happened. Ad-hoc UART loops only
as a panic or trap floor.

**Drift.** When an RFC or ADR contradicts the code, or two docs contradict each other, do not
silently rewrite history: add a short "Drift" note to the RFC/ADR and record the competing readings
so the direction is decided. This matters most for syscall errors, capability transfer and IPC.

## The gate

`just ledger-gate` runs `nx ledger lint`; it is part of `just check`, which CI runs. For docs it
rejects:

- a missing or malformed frontmatter, a key out of canonical order;
- a `sources` path or `contracts` id that does not exist;
- a dead backticked path or markdown link;
- task-status words in prose;
- a page over its word budget;
- a stale generated block;
- a `docs/<path>.md` named in a code comment (Rust, shell, TOML, Python, YAML, Markdown under `source/`,
  `userspace/`, `tools/`, `scripts/`, `recipes/`, `policies/`, `.claude/`, `.github/`) that does not
  exist.

`nx ledger lint --fix` renders canonical frontmatter and generated blocks; it rewrites every file it
touches.
