---
title: TRACK Dev Studio (IDE): Xcode/Qt-Creator-class app builder + debugger + store submission (developer ecosystem keystone)
status: Draft
owner: @devx @ui @runtime
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DevX nx CLI (scaffold/inspect/postflight): tasks/TASK-0045-devx-nx-cli-v1.md
  - DSL state/nav core: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - DSL service stubs: tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - SDK workflow (typed clients/templates): tasks/TASK-0164-sdk-v1-part2a-typed-clients-app-templates-nx-sdk.md
  - SDK workflow (pack/sign/CI): tasks/TASK-0165-sdk-v1-part2b-dev-workflow-lints-pack-sign-ci-host.md
  - SDK workflow (OS install/launch proofs): tasks/TASK-0166-sdk-v1-part2b-os-local-catalog-install-launch-proofs.md
  - Packaging toolchain (bundle authoring/signing): tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - App Store umbrella (publishing target): tasks/TRACK-APP-STORE.md
  - Zero-Copy App Platform (intents/grants): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
---

## Goal (track-level)

Deliver a first-party **Dev Studio** (IDE) that proves “developers can build and ship apps on Nexus”:

- create projects from templates (apps/services/tests),
- edit + run host tests fast,
- build/pack/sign bundles deterministically,
- deploy to device/emulator and run postflights,
- and **submit** apps to the Store publishing pipeline (initially local/offline).

This is intentionally big: it is the “Xcode/Qt Creator” anchor that turns the OS into a self-sustaining platform.

## Scope boundaries (anti-drift)

- v0 is not “clone VSCode”. Focus on the Nexus workflow end-to-end.
- No ambient access: Dev Studio must use explicit capabilities (build tooling, device deploy, signing keys).
- Do not invent a second packaging or build system: Dev Studio orchestrates existing canonical tools (`nx`, `pkgr`, postflights).

## Product stance

- **Side panel**: project tree, targets, run configurations, issues.
- **Main editor**: code + DSL views (including previews/snapshots).
- **Bottom panel**: build output, tests, device logs, debugger console.
- **One-click**: Run, Test, Pack, Install, Submit.

## Architecture stance

Dev Studio is a composition/orchestrator:

- it shells out to `nx` in a controlled, bounded way (host-first),
- it reads structured outputs (`--json`) for deterministic UI,
- and it reuses the same proof mechanisms as CI (no fake success).

## Core features by phase

### Phase 0 — Host-first IDE MVP (fast loop)

- open workspace, project tree, basic editor
- `nx` integration:
  - scaffold from templates
  - run tests (`cargo test` and `nx postflight` wrappers)
  - pack/sign (delegates to packaging tools)
- diagnostics panel: parse bounded compiler/test output into structured issues

### Phase 1 — OS deploy + debug loop

- install/launch via the canonical OS install path (SDK v1 part2b)
- view device logs (bounded)
- minimal debugger loop (attach, backtrace, symbols) — exact mechanism to align with existing crashdump tooling tasks

### Phase 2 — Store submission workflow (offline-first)

- generate Store-ready metadata (manifest fields, screenshots, permissions)
- run static checks:
  - policy caps review
  - signature/trust verification
  - SBOM presence and license allowlist summary (via supply chain tasks)
- “Submit”:
  - v0: publish into a local store feed/channel (for development)
  - later: publish into remote channels (capability-gated)

## Security invariants (hard)

- No secrets in logs (signing keys, tokens).
- Signing keys are mediated by keystore tooling; Dev Studio never prints private material.
- Submission artifacts are content-addressed/hashed; uploads/publishing is auditable.
- Deterministic outputs: project templates and generated metadata must be stable.

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-DEVSTUDIO-000: IDE shell v0 (project tree + editor + panels)**
- **CAND-DEVSTUDIO-010: nx integration v0 (scaffold/test/pack/sign; structured outputs)**
- **CAND-DEVSTUDIO-020: OS deploy v0 (install/launch + logs; bounded)**
- **CAND-DEVSTUDIO-030: Store submission v0 (metadata editor + static checks + local publish)**
