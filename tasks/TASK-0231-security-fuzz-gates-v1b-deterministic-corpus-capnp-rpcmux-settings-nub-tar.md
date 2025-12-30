---
title: TASK-0231 Security fuzz gates v1b (host-first): deterministic corpus tests for Cap’n Proto/RPC boundaries + settings + NUB tar path safety (offline)
status: Draft
owner: @security
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Fuzzing v1 (cargo-fuzz smoke): tasks/TASK-0190-fuzzing-v1-deterministic-harness-pack-html-css-png-ttf-nxb.md
  - Settings v2 (parser surface): tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - Packaging NXB parser surface: tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - RPC/Mux boundaries (if present): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
---

## Context

We already track deterministic fuzz smoke for several heavy parsers via `cargo-fuzz` (`TASK-0190`).
This prompt asks for “fuzz gates” without external engines; the closest repo-native analogue is:

- deterministic **corpus-driven boundary tests**: iterate fixed byte vectors through parsers/decoders and assert:
  - no panics,
  - bounded allocations,
  - deterministic errors,
  - critical invariants (e.g., no path traversal).

This complements (does not replace) `cargo-fuzz` smoke, and is host-first by design.

## Goal

Add deterministic, offline “fuzz-ish” gates (plain `cargo test`) for these risk surfaces:

1. **Cap’n Proto / RPC boundary decode** (oversize, truncated, malformed frames)
2. **Settings input validation** (wrong JSON types, deep nesting, oversize strings)
3. **NUB/TAR extraction safety** (reject `..`, absolute paths, symlink tricks, size overflows) when that surface exists

## Non-Goals

- Running fuzzing in QEMU/OS.
- Claiming corpus tests are exhaustive fuzzing; they are regression gates only.
- Introducing new formats or ad-hoc frame contracts.

## Constraints / invariants (hard requirements)

- Deterministic seeds/cases; stable ordering.
- Inputs bounded and stored in repo (small).
- Tests must assert “no panic” and specific deterministic error codes/messages (bounded).

## Red flags / decision points

- **YELLOW (surface availability)**:
  - If a target surface is not implemented yet (e.g., NUB tar extraction), add the test harness scaffold and
    keep it failing/ignored **only if** clearly marked as blocked by a prerequisite task.

## Stop conditions (Definition of Done)

- `cargo test -p security_fuzz_gates_host -- --nocapture` (new) passes:
  - corpus cases run for each surface,
  - zero panics,
  - bounded rejection behavior proven.

## Touched paths (allowlist)

- `tests/security_fuzz_gates_host/` (new)
- `docs/security/fuzzing.md` (extend: corpus gates vs cargo-fuzz)
- target crates under `source/` / `userspace/` only as needed to expose safe decode entry points

## Plan (small PRs)

1. Add corpus harness crate + first suite (Cap’n Proto/RPC boundary).
2. Add settings suite once `settingsd` parser entry points exist.
3. Add tar safety suite once the NUB/tar extractor surface exists (or as soon as it is introduced).

## Acceptance criteria (behavioral)

- Deterministic corpus gates reliably catch panics and unsafe decode behavior in high-risk parsers without relying on non-deterministic fuzz infrastructure.
