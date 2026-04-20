<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Proof Manifest schema (`source/apps/selftest-client/proof-manifest.toml`)

- Status: TASK-0023B is `In Review` 2026-04-20; all six phases functionally closed (P1 extraction → P2 two-axis → P3 arch-gate → P4 manifest SSOT → P5 schema-v2 split + signed evidence → P6 replay/diff/bisect). RFC-0038 is `Done`. Single remaining environmental closure step: external CI-runner replay artifact for P6-05, see [`docs/testing/replay-and-bisect.md`](replay-and-bisect.md) §7-§11.
- Owners: @runtime
- Anchor RFC: [RFC-0038](../rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md)
- Phase-list contract: [RFC-0014 v2](../rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md) §3 (extended 8 → 12 in Cut P2-00).
- Parser crate: [`source/libs/nexus-proof-manifest/`](../../source/libs/nexus-proof-manifest/).

## Why this file exists

Before Phase 4 the `selftest-client` marker ladder was implicitly co-owned by two surfaces that drifted independently:

1. [`scripts/qemu-test.sh`](../../scripts/qemu-test.sh) — hard-coded `PHASES`/`PHASE_START_MARKER`/`REQUIRE_*` arrays that the harness used to gate UART expectations.
2. [`source/apps/selftest-client/src/`](../../source/apps/selftest-client/src/) — the actual emitter, with `"SELFTEST: …"` string literals scattered across 17 source files (allowlisted in `.arch-allowlist.txt` `[marker_emission]`).

`proof-manifest.toml` collapses these two truths into one declarative artifact. From Cut P4-04 onward the emitter references constants generated from this manifest (`crate::markers_generated::M_*`). From Cut P4-05 onward the harness reads its expectations from the manifest via a thin host CLI binary (`nexus-proof-manifest list-markers --profile=…`). After Phase-4 closure, "add a new marker" means "add a `[marker."…"]` entry"; nothing else.

This document is the **normative schema**. The TOML file is the data; this Markdown file is the contract that data must obey.

## Schema-level invariants (Phase-4 carry-forward)

- Schema is **closed**: any unknown top-level key, unknown `[meta]` key, or unknown `[phase.*]` key rejects with a stable [`ParseError`](../../source/libs/nexus-proof-manifest/src/error.rs) variant. Additions require a schema bump (`[meta] schema_version`) and a coordinated parser update.
- Phase-name set MUST equal RFC-0014 §3 v2 (12 phases). The 1:1 binding table is in §"Phase mapping" below (populated in Cut P4-02).
- Default profile MUST exist as a `[profile.X]` block (validated at parse time from Cut P4-05).
- No `parking_lot`/`getrandom` are pulled into the OS graph: the parser is a host-only crate, consumed via `[build-dependencies]` (P4-03+) and from host tools (`scripts/qemu-test.sh`, `tools/os2vm.sh`).

## Top-level shape

```toml
[meta]
schema_version = "1"          # required, non-empty
default_profile = "full"      # required, non-empty; must resolve to a [profile.X]

[phase.<name>]                # 12 blocks, exactly one per RFC-0014 phase
order = <1..12>               # required, unique across the manifest

[profile.<name>]              # P4-05+ populates runner/env/extends/phases
# (skeleton at P4-01: header only, body added in later cuts)

[marker."<literal>"]          # P4-03+ populates one entry per emitted marker
# phase = "<phase>"
# proves = "..."              # optional, free-text
# introduced_in = "TASK-..."  # optional, traceability
# emit_when = { profile = "<name>" }      # optional (P4-08)
# emit_when_not = { profile = "<name>" }  # optional (P4-08)
# forbidden_when = { profile = "<name>" } # optional (P4-09 deny-by-default)
```

## Reject categories (Cut P4-01 surface)

The skeleton parser surfaces 6 reject categories. Each maps 1:1 to a [`ParseError`](../../source/libs/nexus-proof-manifest/src/error.rs) variant, and each has a dedicated test in [`tests/parse_skeleton.rs`](../../source/libs/nexus-proof-manifest/tests/parse_skeleton.rs):

| Variant                          | Trigger                                                     |
| -------------------------------- | ----------------------------------------------------------- |
| `Toml(_)`                        | Syntactically invalid TOML (handled before any schema check). |
| `MissingMeta`                    | `[meta]` table absent.                                      |
| `MissingSchemaVersion`           | `[meta].schema_version` missing or empty.                   |
| `MissingDefaultProfile`          | `[meta].default_profile` missing or empty.                  |
| `UnknownTopLevelKey(k)`          | A top-level key other than `meta`/`phase`/`profile`/`marker`. |
| `UnknownMetaKey(k)`              | A key inside `[meta]` other than the two whitelisted ones.  |
| `DuplicatePhase(name)`           | Two `[phase.X]` blocks with the same name.                  |
| `PhaseOrderConflict { … }`       | Two `[phase.X]` blocks with the same `order`.               |

(Marker / profile / runtime-profile reject categories land in P4-03 / P4-05 / P4-08 respectively.)

## Phase mapping (RFC-0014 ↔ manifest ↔ source)

This mapping is **normative**: every row must hold simultaneously. Adding a phase requires updating all three columns in the same change (RFC-0014 §3, this table, the source file). Deleting a phase requires the same plus a `schema_version` bump.

| Order | RFC-0014 §3 phase | `[phase.X]` block | `os_lite/phases/<x>.rs`                                                                                                       |
| ----- | ----------------- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------- |
|     1 | bringup           | `bringup`         | [`source/apps/selftest-client/src/os_lite/phases/bringup.rs`](../../source/apps/selftest-client/src/os_lite/phases/bringup.rs)         |
|     2 | ipc_kernel        | `ipc_kernel`      | [`source/apps/selftest-client/src/os_lite/phases/ipc_kernel.rs`](../../source/apps/selftest-client/src/os_lite/phases/ipc_kernel.rs) |
|     3 | mmio              | `mmio`            | [`source/apps/selftest-client/src/os_lite/phases/mmio.rs`](../../source/apps/selftest-client/src/os_lite/phases/mmio.rs)               |
|     4 | routing           | `routing`         | [`source/apps/selftest-client/src/os_lite/phases/routing.rs`](../../source/apps/selftest-client/src/os_lite/phases/routing.rs)         |
|     5 | ota               | `ota`             | [`source/apps/selftest-client/src/os_lite/phases/ota.rs`](../../source/apps/selftest-client/src/os_lite/phases/ota.rs)                 |
|     6 | policy            | `policy`          | [`source/apps/selftest-client/src/os_lite/phases/policy.rs`](../../source/apps/selftest-client/src/os_lite/phases/policy.rs)           |
|     7 | exec              | `exec`            | [`source/apps/selftest-client/src/os_lite/phases/exec.rs`](../../source/apps/selftest-client/src/os_lite/phases/exec.rs)               |
|     8 | logd              | `logd`            | [`source/apps/selftest-client/src/os_lite/phases/logd.rs`](../../source/apps/selftest-client/src/os_lite/phases/logd.rs)               |
|     9 | vfs               | `vfs`             | [`source/apps/selftest-client/src/os_lite/phases/vfs.rs`](../../source/apps/selftest-client/src/os_lite/phases/vfs.rs)                 |
|    10 | net               | `net`             | [`source/apps/selftest-client/src/os_lite/phases/net.rs`](../../source/apps/selftest-client/src/os_lite/phases/net.rs)                 |
|    11 | remote            | `remote`          | [`source/apps/selftest-client/src/os_lite/phases/remote.rs`](../../source/apps/selftest-client/src/os_lite/phases/remote.rs)           |
|    12 | end               | `end`             | [`source/apps/selftest-client/src/os_lite/phases/end.rs`](../../source/apps/selftest-client/src/os_lite/phases/end.rs)                 |

The 1:1 binding gives Phase-4 cuts a single review surface: a manifest review can confirm that every declared `[phase.X]` block has a matching RFC-0014 phase and a matching source file, and a phase-source review can confirm that every emitted phase-ok marker is declared in the manifest. Both reviews refer back to this table.

## Runtime-vs-harness profile distinction (Cut P4-08 preview)

Phase 4 introduces two disjoint kinds of profile:

- **Harness profiles** (`full`, `smp`, `dhcp`, `os2vm`, `quic-required`) — consumed by [`scripts/qemu-test.sh`](../../scripts/qemu-test.sh) and [`tools/os2vm.sh`](../../tools/os2vm.sh) to decide which QEMU runner / env / required markers apply. These shape the **outside** of the run.
- **Runtime profiles** (`full`, `bringup`, `quick`, `ota`, `net`, `none`) — consumed by [`source/apps/selftest-client/src/os_lite/profile.rs`](../../source/apps/selftest-client/src/os_lite/) (added in P4-08) to decide which phases the binary itself emits. These shape the **inside** of the run.

Runtime-only profiles MUST set `runtime_only = true` (validated from P4-08); harness profiles MUST NOT. The `full` profile is the only legal name in both spaces — by construction it means the same thing on each side (every phase, no env extras).

## How to extend (post-Phase-4)

- **New marker** → add a `[marker."…"]` entry; `build.rs` re-emits `markers_generated.rs` at next compile; the emission site references the generated constant; `arch-gate` rule 3 enforces the literal does not appear elsewhere.
- **New profile** → add a `[profile.X]` block (with `extends` if it inherits); `qemu-test.sh --profile=X` and `just test-os PROFILE=X` work without script changes.
- **New phase** → update RFC-0014 §3 first; add `[phase.X]` block here; add `os_lite/phases/<x>.rs`; register in `os_lite/profile.rs::dispatch_phase` (P4-08+).

(See P4-10 closure docs in [`docs/testing/index.md`](index.md) for the full workflow once Phase 4 lands.)
