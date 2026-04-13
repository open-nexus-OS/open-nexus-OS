---
title: TASK-0078B DSL v0.2b QuerySpec v1: foundation + service-gated execution + syntax/paging/hash floor + host proofs
status: Draft
owner: @ui
created: 2026-04-03
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL query posture: docs/dev/dsl/db-queries.md
  - DSL services/effects posture: docs/dev/dsl/services.md
  - DSL state tiers: docs/dev/dsl/state.md
  - DSL v0.2b stubs + demo baseline: tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
  - DSL v0.1 CLI baseline: tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - DSL v0.2a core: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - QuerySpec v2 hardening: tasks/TASK-0274-dsl-v0_2c-db-query-objects-builder-defaults-paging-deterministic.md
  - QuerySpec v3 lazy data surfaces: tasks/TASK-0275-ui-v5c-lazy-data-loading-virtual-list-paging-contract.md
  - Document picker consumer: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Files consumer: tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Zero-Copy App Platform (connectors/query IR): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0078` gives the DSL real effect-side service calls via typed stubs. That is the right layer to establish the first
usable **QuerySpec foundation** so early data-heavy surfaces do not wait for later ergonomics/hardening tasks.

We want:

- a first-class structured query value in DSL/runtime code,
- a minimal syntax surface that is already usable in host demos and real app tasks,
- deterministic canonicalization/hashing and a small paging floor,
- execution only through typed service stubs/effects,
- and a base that early consumers such as Document Picker and Files can extend with domain-specific helpers without
  inventing their own query core.

This task is intentionally the **v1 foundation** only. Stronger defaults, richer builder ergonomics, and hardening stay in
`TASK-0274`. Large lazy/virtualized data surfaces stay in `TASK-0275`.

## Goal

Deliver host-first support for:

1. **QuerySpec v1 core in IR/runtime**:
   - first-class `IrQuerySpec` / runtime query value
   - fields sufficient for v1 floor:
     - source/table handle
     - predicates (v1 minimal: equality only)
     - ordering
     - limit
     - opaque page token
   - query values are immutable/persistent in semantics
2. **Minimal DSL syntax floor**:
   - enough syntax to create and pass QuerySpec values in DSL code
   - syntax may be smaller than the later v2 builder, but must already support:
     - selecting a root/source
     - equality predicates
     - explicit ordering
     - explicit limit
     - explicit page token passing
3. **Canonicalization + hash floor**:
   - identical logical QuerySpec inputs produce identical canonical form + hash across runs
   - canonical form is stable enough for host proofs, caches, and query equality checks
4. **Paging floor**:
   - `PageToken` is an opaque value passed through DSL/runtime/stubs unchanged
   - request/response contracts can carry `next`/continuation values deterministically
   - no ad-hoc timer- or offset-based paging behavior in the DSL layer
5. **Service-gated execution only**:
   - QuerySpec can be passed to typed stubs and executed only in effects/services
   - reducers/composables may build/manipulate QuerySpec as pure values
   - UI never opens DB files or executes ad-hoc query strings
6. **Host proofs + one real consumer path**:
   - extend the demo/test path so QuerySpec is exercised through the effect runner and stub registry
   - prove at least one realistic query-shaped flow (e.g. master-detail list source or a small content-style fixture)

## Non-Goals

- Full QuerySpec v2 ergonomics/defaults/hardening; that remains in `TASK-0274`.
- Lazy-loading/viewport/provider contracts; that remains in `TASK-0275`.
- Joins, OR, ranges, full-text search, or arbitrary SQL/GraphQL text.
- A new DB authority or direct UI-owned storage engine.

## Constraints / invariants (hard requirements)

- **Pure vs IO split**:
  - building/manipulating QuerySpec is pure,
  - execution is effect-only and service-gated.
- **Determinism**:
  - canonicalization/hashing must be stable across runs,
  - ordering and page token propagation must be explicit and deterministic.
- **Bounds**:
  - explicit row/byte/time caps remain enforced at the service boundary,
  - the DSL/runtime must not encourage unbounded result handling.
- **No authority drift**:
  - QuerySpec is a value contract, not a license for direct DB access from UI/runtime code.
- **Extension posture**:
  - early consumers (Document Picker, Files, similar content/provider surfaces) may add domain-specific helpers or presets
    on top of QuerySpec,
  - but they must not fork canonicalization, execution rules, or invent a parallel query core.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Proof (Host) — required

`tests/dsl_queryspec_v1_host/`:

- the same logical QuerySpec yields the same canonical form/hash across runs
- QuerySpec built in pure DSL/store code is passed to a typed stub only from an effect
- mock stub receives predicates/order/limit/page token deterministically
- a paged response with `next` token roundtrips deterministically through runtime state
- one consumer-shaped fixture proves that app-facing helpers can build on the shared QuerySpec core without changing its
  execution rules

## Touched paths (allowlist)

- `userspace/dsl/nx_syntax/` (extend: minimal QuerySpec syntax)
- `userspace/dsl/nx_ir/` (extend: QuerySpec core + canonicalization/hash floor)
- `userspace/dsl/nx_interp/` (extend: runtime value model + effect-side transport)
- `userspace/dsl/nx_stubs/` (extend: typed stub QuerySpec parameter support)
- `tests/dsl_queryspec_v1_host/` (new)
- `docs/dev/dsl/db-queries.md` + `docs/dev/dsl/services.md`

## Plan (small PRs)

1. QuerySpec core + canonical form/hash floor in IR/runtime
2. minimal DSL syntax + effect/stub transport
3. host tests + demo/consumer-shaped fixture + docs
