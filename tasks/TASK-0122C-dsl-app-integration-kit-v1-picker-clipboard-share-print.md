---
schema: 1
id: TASK-0122C
title: "DSL App Integration Kit v1: picker + clipboard + share + print bridges for apps"
status: draft
owner: ["@ui"]
created: 2026-03-28
subsystem: messaging
split_from: TASK-0122
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "DSL App Platform v1"
    target: tasks/TASK-0122B-dsl-app-platform-v1-shell-routing-launch-contract.md
  - label: "Document picker/openWith"
    target: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - label: "Clipboard baseline"
    target: tasks/TASK-0067-ui-v7b-dnd-clipboard-v2.md
  - label: "Share target baseline"
    target: tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - label: "Print pipeline"
    target: tasks/TASK-0088-ui-v13b-print-to-pdf-printd-preview.md
---

## Context

Even with a shared app shell, app development will still drift if every app reinvents picker, clipboard, share, print,
and recents bridges. Files/Text/Images/Notes should consume one deterministic app-facing integration kit.

## Goal

Deliver:

1. Shared DSL effect-side adapters:
   - picker open/save
   - clipboard read/write
   - share/send target
   - print
   - optional recents/open-history helpers
2. Canonical placement:
   - reusable shared adapters live in DSL bridge/platform packages
   - app-local wrappers live in `ui/services/**.service.nx`
3. Deterministic mocks and fixtures for host tests:
   - apps can test integrations without live services
4. Documentation for app authors:
   - how to call integrations from `@effect`
   - what never belongs in reducers/views

## Non-Goals

- Replacing the underlying services.
- Implementing app-specific behavior.
- Full e2e UI coverage for every app in this task.

## Constraints / invariants (hard requirements)

- All service interaction stays in effects or bridge layers.
- No duplicate mini-bridges inside every app.
- Integration kit behavior must be deterministic under fixtures.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- picker/clipboard/share/print fixtures are stable
- example app effects can round-trip through the kit deterministically

## Touched paths (allowlist)

- DSL bridge/platform packages
- `tests/dsl_app_integration_kit_host/` (new)
- `docs/dev/dsl/overview.md`
- `docs/dev/dsl/cli.md`
- app task docs that consume the kit

## Plan (small PRs)

1. shared adapters + mocks
2. example app integration tests
3. docs + task handoffs
