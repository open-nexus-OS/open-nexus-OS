---
title: TASK-0275 UI v5c (host-first): lazy data loading contract (virtual list ↔ pager ↔ query objects) + deterministic proofs
status: Draft
owner: @ui
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Virtualized list widget: tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - Scroll/clip foundation: tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - DSL query objects: tasks/TASK-0274-dsl-v0_2c-db-query-objects-builder-defaults-paging-deterministic.md
  - DSL v0.2 app mechanics: tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
---

## Context

We already plan:

- deterministic scroll/clip (`TASK-0059`)
- a virtualized list with recycling (`TASK-0063`)
- DSL effects + service calls

What’s missing is a **single deterministic “lazy loading contract”** so apps/SystemUI can load large datasets
without unbounded memory, unbounded network/service calls, or flaky timing.

## Goal

Define and prove a v5c contract:

1. **Paged data model**:
   - `Page<T> { items: Vec<T>, next: Option<PageToken>, truncated: bool }`
   - stable, bounded sizes
2. **Lazy provider interface** (host-first; used by DSL interpreter and native UI):
   - `ItemProvider` supports:
     - `len_hint() -> Range` (optional, bounded)
     - `get(index_range) -> items` (may return placeholders until loaded)
     - `request_more(trigger)` (deterministic; no wallclock)
3. **Virtual list integration**:
   - virtualization asks for visible range
   - when visible range crosses a deterministic threshold (e.g. last visible index ≥ loaded_count - K),
     it triggers `request_more(ReachedEndThreshold)`
   - K is fixed/configured; must not depend on frame timing
4. **Effect scheduling**:
   - lazy loading requests are expressed as effects (post-reducer commit)
   - bounded concurrency: at most 1 in-flight page request per provider

## Non-Goals

- Infinite scroll “based on timers” or heuristics.
- Prefetch algorithms that depend on measured throughput.
- Unbounded caching of pages/items.

## Constraints / invariants (hard requirements)

- Deterministic triggers (viewport/index based, not time based).
- Bounded memory (cap pages kept, cap placeholders).
- No fake success: UI shows a loading placeholder when not loaded; never shows “complete list” if truncated.

## Proof (Host) — required

`tests/ui_v5c_lazy_host/`:

- scrolling a viewport over 1000 items triggers bounded page requests deterministically
- only one in-flight load at a time; duplicate triggers coalesce
- virtualization range + recycling remains stable under repeated scroll sequences

## Touched paths (allowlist)

- `userspace/ui/widgets/virtual_list/` (extend: provider hooks)
- `userspace/ui/runtime/` (extend: effect scheduling integration hooks)
- `tests/ui_v5c_lazy_host/`
- `docs/ui/lazy-loading.md`
