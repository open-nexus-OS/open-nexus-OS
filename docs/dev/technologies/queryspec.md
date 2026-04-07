<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# QuerySpec

QuerySpec is the shared query model for app surfaces that need structured filter/order/page state without inventing their
own mini database API.

## Primary task anchors

- `tasks/TASK-0078B-dsl-v0_2b-queryspec-v1-foundation-service-gated-paging-hash.md`
- `tasks/TASK-0274-dsl-v0_2c-db-query-objects-builder-defaults-paging-deterministic.md`
- `tasks/TASK-0275-ui-v5c-lazy-data-loading-virtual-list-paging-contract.md`

## Good fit

Use QuerySpec when your app has:

- structured list, picker, history, or table-like surfaces,
- durable filter/order/page state that should remain pure,
- or data-heavy UI that should stay deterministic across lazy loading and virtualization.

Typical consumers:

- Files,
- document picker,
- browser history/downloads,
- feeds and social timelines backed by local caches,
- office-style tables and result views,
- maps/bookmark/history-like surfaces.

## What it gives app developers

- one consistent way to model query state,
- cleaner separation between pure UI state and effect-side execution,
- deterministic paging and equality/canonicalization posture,
- and a shared foundation across multiple apps instead of one-off query builders everywhere.

## Best practice

- build QuerySpec in reducers/composables/store code as a pure value,
- execute it only through service-gated effects,
- pair it with lazy-loading/provider contracts for large result sets,
- and keep command flows such as rename/delete/send/import outside the query model.

## Avoid

- imperative mutation commands,
- ad-hoc direct DB access from UI code,
- or unbounded raw query text embedded in app surfaces.

## Roadmap posture

- v1: foundation, syntax floor, paging token transport, canonicalization/hash floor
- v2: builder ergonomics, defaults, diagnostics, stronger deterministic posture
- v3: lazy data surfaces, virtual list/provider integration, placeholder and paging consumption

## Related docs

- `docs/dev/dsl/db-queries.md`
- `docs/dev/dsl/services.md`
- `docs/dev/ui/collections/lazy-loading.md`
