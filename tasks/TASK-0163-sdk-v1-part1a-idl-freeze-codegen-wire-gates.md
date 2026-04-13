---
title: TASK-0163 SDK v1 Part 1a (host-first): freeze Cap'n Proto IDLs + deterministic codegen + SemVer/wire gates
status: Draft
owner: @devx
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DevX nx CLI v1: tasks/TASK-0045-devx-nx-cli-v1.md
  - Existing schemas: tools/nexus-idl/schemas/
  - Existing runtime generator: userspace/nexus-idl-runtime/
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want a developer SDK with a stable public control-plane API surface.
Repo reality today:

- Cap’n Proto schemas live in `tools/nexus-idl/schemas/*.capnp`.
- `userspace/nexus-idl-runtime/build.rs` compiles those schemas into `OUT_DIR` **or** falls back to checked-in `src/manual/*`.
- The `tools/nexus-idl` CLI is a stub.
- File discovery order is not deterministic enough for “wire goldens” gates (e.g. directory iteration order).

This task establishes **SDK v1 IDL freeze** and deterministic gates (host-first). App templates and scaffolding live in `TASK-0164`.

## Goal

Deliver:

1. IDL freeze:
   - create `sdk/idl/v1/` as the canonical SDK API surface
   - freeze the set of IDLs that **exist today**:
     - `bundlemgr.capnp`, `dsoftbus.capnp`, `execd.capnp`, `identity.capnp`, `keystored.capnp`,
       `packagefs.capnp`, `policyd.capnp`, `samgr.capnp`, `vfs.capnp`
   - add `sdk/idl/v1/README.md` with:
     - naming conventions (`nexus.v1.<service>`)
     - ID stability rules (no reuse; gaps allowed)
     - evolution rules (additive changes OK; breaking changes require v2 directory)
     - reserved ID ranges (public vs reserved)
2. Deterministic codegen pipeline:
   - a host tool that:
     - enumerates `sdk/idl/v1/**/*.capnp` in **sorted** order
     - runs capnp codegen reproducibly (no absolute host paths in outputs)
     - writes generated Rust to a stable repo-local path (or stable `target/generated/...` with a check mode)
   - update `userspace/nexus-idl-runtime/build.rs` to:
     - prefer `sdk/idl/v1/` as input (with compatibility fallback to `tools/nexus-idl/schemas` during transition)
     - enumerate inputs in sorted order (deterministic)
3. Golden artifacts + gates:
   - `sdk/golden/idl-v1.tar.zst` (sorted filenames + normalized mtimes)
   - `tools/idl-diff`:
     - classify schema changes as OK vs BREAK under v1 rules
   - `tools/wire-verify`:
     - decode/encode golden request/response vectors and enforce byte equality
   - `ci/sdk_gates.sh`:
     - runs deterministic codegen → idl-diff → wire-verify
4. Host tests:
   - verify gates detect a simulated BREAK
   - verify vectors are stable byte-for-byte

## Non-Goals

- Kernel changes.
- OS/QEMU proof (this is a host-only SDK workflow task).
- Freezing IDLs that do not exist yet (IME/search/media/backup/etc. are added when implemented).

## Constraints / invariants (hard requirements)

- Determinism: sorted schema enumeration; stable archives; stable vectors.
- No fake success: gate must fail on any incompatible v1 change.
- No `unwrap/expect`; no blanket `allow(dead_code)` in tools.

## Red flags / decision points (track explicitly)

- **YELLOW (two schema roots today)**:
  - `tools/nexus-idl/schemas` vs `sdk/idl/v1`. We must pick `sdk/idl/v1` as canonical to avoid drift,
    and treat `tools/nexus-idl/schemas` as either:
    - a mirror of the canonical SDK IDLs, or
    - a transitional alias that is removed later.

- **YELLOW (generated code location)**:
  - Decide whether generated Rust is checked in under `sdk/generated/...` (stable, reviewable diffs)
    or produced under `target/` with a strict `--check` mode.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `./ci/sdk_gates.sh`
    - `cargo test -p sdk_v1_part1_host -- --nocapture` (or equivalent)
  - Required proofs:
    - idl-diff classifies an intentional breaking change as BREAK
    - wire-verify passes for committed golden vectors

## Touched paths (allowlist)

- `sdk/idl/v1/` (new)
- `sdk/golden/` (new)
- `tools/idl-diff/` (new)
- `tools/wire-verify/` (new)
- `ci/sdk_gates.sh` (new)
- `userspace/nexus-idl-runtime/` (sorted enumeration + sdk root)
- `tools/nexus-idl/` (optional: can be upgraded or left as stub; SDK tools become canonical)

## Plan (small PRs)

1. IDL freeze directory + README + golden archive creation
2. Deterministic codegen tool + update `nexus-idl-runtime` to use sorted inputs from `sdk/idl/v1`
3. idl-diff + wire-verify tools + golden vectors
4. CI script + host tests

## Acceptance criteria (behavioral)

- Running `ci/sdk_gates.sh` is deterministic and fails on any breaking v1 schema drift.
