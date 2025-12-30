---
title: TASK-0165 SDK v1 Part 2a (host-first): nx sdk dev-tools + templates + manifest lints + pack/sign workflow + CI gates
status: Draft
owner: @devx
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - SDK v1 Part 1a gates: tasks/TASK-0163-sdk-v1-part1a-idl-freeze-codegen-wire-gates.md
  - SDK v1 Part 1b clients/templates: tasks/TASK-0164-sdk-v1-part1b-nx-sdk-templates-client-stubs.md
  - DevX nx CLI v1 (canonical CLI): tasks/TASK-0045-devx-nx-cli-v1.md
  - Packages v1 tooling (pkgr, manifest.nxb): tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - Identity/Keystore v1.1 tooling (nx key): tasks/TASK-0159-identity-keystore-v1_1-host-keystored-lifecycle-nonexportable.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

SDK v1 Part 1 establishes IDL freeze + codegen + compatibility gates (`TASK-0163/0164`).
Part 2 completes the developer workflow:

- `nx sdk` commands (doctor/build/run/test/pack/sign),
- service template scaffolding,
- manifest/permissions lints and schema validation,
- CI pipeline integration and failure summaries.

Important repo reality:

- The packaging toolchain is currently inconsistent:
  - `tools/nxb-pack` writes `manifest.json` while docs define `manifest.nxb` as canonical.
  - `tools/pkgr` is still a placeholder.
So “pack/sign” must be **gated** on the packages tooling task that makes `pkgr` real (`TASK-0129`).

## Goal

Deliver, host-first:

1. Canonical CLI surface:
   - implement SDK commands as `nx sdk ...`
   - optionally provide `nx-sdk` as a thin shim forwarding to `nx sdk` (no duplicate logic)
2. `nx sdk` commands:
   - `doctor`: checks toolchain, `capnp`, SDK gates readiness, workspace sanity
   - `build/test/run`: deterministic wrappers around `cargo` (stable env; forward exit codes)
   - `pack/sign` (gated):
     - `pack` creates a deterministic `.nxb` using the canonical `pkgr`/pack tooling from `TASK-0129`
     - `sign` uses `nx key`/keystored signing once available; outputs deterministic signature placement
3. Templates:
   - service template (daemon skeleton) in `sdk/templates/service-skeleton/`
   - app template already exists from Part 1b; both templates must build/test deterministically
4. Manifest/permissions lints:
   - implement `nx manifest lint` (or `nx sdk lint`) using JSON schemas
   - checks: missing icons/locales, invalid caps, SemVer regression, wildcard caps (dev-mode only)
5. CI gates:
   - `just sdk-gates` runs:
     - idl-gen + idl-diff + wire-verify
     - template build/test
     - pack determinism check (only once `TASK-0129` is done)
   - produce a concise summary artifact with pass/fail reasons

## Non-Goals

- OS/QEMU proof (catalog/install/launch is in Part 2b and is OS-gated).
- Replacing the packages toolchain; we integrate with it once it exists.

## Constraints / invariants (hard requirements)

- Deterministic output and stable logs.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success:
  - `pack/sign` must be disabled or explicitly error with “blocked by TASK-0129” until the toolchain is real.

## Red flags / decision points (track explicitly)

- **RED (packaging drift)**:
  - SDK must not invent a third packaging format. Use canonical `manifest.nxb` + `.nxb` tooling from `TASK-0129`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Commands:
    - `./ci/sdk_pipeline.sh` (or `just sdk-gates` + template checks)
  - Required:
    - doctor/build/test/template scaffolding deterministic and green
    - lint tool catches bad fixtures deterministically
    - pack determinism test passes once `TASK-0129` is complete

## Touched paths (allowlist)

- `tools/nx/` (add `sdk` subcommands)
- `tools/nx-sdk/` (optional shim only)
- `sdk/templates/service-skeleton/` (new)
- `tools/nx-manifest-lint/` (optional dedicated crate if not built into `nx`)
- `ci/` (sdk pipeline script)
- `docs/sdk/` (Part 2 docs)
- `tests/sdk_v1_part2_host/` (new)

## Plan (small PRs)

1. Add service template + `nx sdk new service ...`
2. Add manifest lint + schemas + host tests
3. Add `nx sdk doctor/build/test/run` wrappers + tests
4. Integrate `pack/sign` once `TASK-0129` is real; add determinism checks + CI gate

## Acceptance criteria (behavioral)

- Developers can use `nx sdk` to scaffold, lint, and build/test apps/services deterministically.
- Pack/sign is either functional and deterministic (when unblocked) or explicitly blocked (no fake success).

