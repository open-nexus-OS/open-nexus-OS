---
schema: 1
id: TASK-0357
title: "Every DSL app is built like an enterprise app: modular pages, components and stores, a README and tests per app, and a gate that keeps it so"
status: done
owner: ["@ui"]
created: 2026-11-09
updated: 2026-11-10
subsystem: app-platform
kind: refactor
novelty: variant
estimate: 2
started: 2026-11-09
done: 2026-11-10
active_days: 2
dates_source: recorded
variance_note: "on estimate (2 of 2 days): the two language changes it needed — slot forwarding and library composables — were seams RFC-0084 had already named, so they cost hours, not days"
goal:
  metric: "DSL apps that pass the app-structure gate (thin pages, components by feature, stores by domain, i18n, README, tests)"
  baseline: "0 of 8 apps pass; 23 baseline entries (6 apps without a README, 15 files over their size, 1 flat component folder, 1 duplicated store); 8 of 8 apps have a conformance test"
  target: "8 of 8 apps pass; 0 entries left in the gate's baseline"
  instrument: tests/dsl_apps_conformance/tests/app_structure.rs
  result: "8 of 8 apps pass the app-structure gate; 0 baseline entries (window store in window-kit, slot forwarding split WinPane and WinAppWindow); every route mounts, every event case reduces (app_smoke), every tap target named (NX0415)"
consumer: TASK-0353/P3
phases:
  - id: P0
    story: "the convention is a gate: docs/dev/dsl/project-layout.md states the app layout (thin route pages, components grouped by feature, one store per domain, every visible string in i18n, a README and tests per app) and tests/dsl_apps_conformance/tests/app_structure.rs measures every app against it; what does not pass yet is listed in a baseline that may only shrink"
    state: done
    gate: "host: app_structure.rs green with its baseline; baseline entries counted per app"
    started: 2026-11-09
    done: 2026-11-09
    evidence: tests/dsl_apps_conformance/tests/app_structure.rs
  - id: P1
    story: "Stash is modular: its page composes feature components, its state is split by domain (files, selection, navigation, the window), and it behaves exactly as before"
    state: done
    gate: "host: Stash's page and stores within the gate's limits (page 100, store 200 logical lines), 0 Stash entries in the baseline; stash.rs and window_menu_global.rs green"
    started: 2026-11-09
    done: 2026-11-09
    evidence: tests/dsl_apps_conformance/tests/stash.rs
  - id: P2
    story: "Settings, Chat, the greeter, the on-screen keyboard, the shell and window-kit are modular: every page within 100 logical lines, every component within 150, every store within 200, components grouped by feature folder"
    state: done
    gate: "host: 0 size or flat-components entries in the baseline; dsl_apps_conformance and nexus-dsl-runtime green"
    started: 2026-11-09
    done: 2026-11-10
    evidence: tests/dsl_apps_conformance/tests/app_structure.rs
  - id: P3
    story: "every app explains and proves itself: a README with purpose, routes, stores and events, services and structure, and tests that mount every route, drive every store's events and check every interactive node's accessible name"
    state: done
    gate: "host: app_structure.rs checks the README sections and an app test file: 8 of 8 apps"
    started: 2026-11-09
    done: 2026-11-10
    evidence: tests/dsl_apps_conformance/tests/app_smoke.rs
  - id: P4
    story: "the window store lives once: a library may ship composables (an RFC amendment to the library governance, today components only), window-kit owns the window chrome store and the apps' copies are gone"
    state: done
    gate: "RFC amendment accepted; host: 0 identical store files across apps, every app compiles, dsl_apps_conformance green"
    started: 2026-11-10
    done: 2026-11-10
    evidence: userspace/apps/window-kit/ui/composables/window.store.nx
---

<!-- `nx ledger new task` copies everything below this line; the frontmatter above shows the
     planning fields (RFC-0115). Write state with `nx ledger`, never by hand. -->

## The ask

The operator, 2026-11-09: every app built and structured like an enterprise-grade app of a modern
component framework, in our DSL — modular, with tests and docs, clean.

## Measured before the cut

`tests/dsl_apps_conformance/tests/app_structure.rs` with an empty baseline, logical lines (not
blank, not a `//` comment): pages median 43 / max 581, components median 42 / p90 128 / max 261,
stores median 74 / p90 188 / max 533. 23 violations: 6 apps without a README (calculator,
desktop-shell, greeter, ime-ui, stash, window-kit), 15 files over their limit (Stash's page 581
and store 533, Settings' page 253, Chat's page 163, the greeter's page 169, four window-kit
components, four shell files, the keyboard's KeyRows, Settings' AppearanceView), the keyboard's
components flat, chat's `window.store.nx` a byte copy of the calculator's (settings carries a
variant without `WinMenuPick`). 8 of 8 apps have a conformance test.

## Three lenses

- **Scope** — in: the convention (`docs/dev/dsl/project-layout.md`), the gate and its baseline,
  the eight DSL apps under `userspace/apps/` (calculator, chat, desktop-shell, greeter, ime-ui,
  settings, stash, window-kit) · out: their design (TASK-0353 P3), the Rust apps demo-exit0 and
  launcher (TASK-0353 P6), the library governance change (P4, behind an RFC amendment).
- **Invariant** — a restructure changes no behaviour: every app's conformance tests stay green
  and its screens render as before; the baseline only shrinks —
  `test_reject_a_file_over_its_limit_and_accept_one_at_it`, and the gate fails on a new
  violation, a grown file and a stale entry.
- **Contract** — extends the app project layout of `docs/dev/dsl/project-layout.md`; libraries
  stay components-only (TASK-0074) until P4's RFC amendment says otherwise.

## Not doing here

The window store in window-kit is P4 and needs the RFC amendment first; the design deltas of each
app are TASK-0353 P3 (its agents restructure an app while they rebuild it); the two Rust apps are
TASK-0353 P6.

## Definition of Done

- Every phase `done` with its evidence; the goal's target reached on the instrument.
- Host tests, the QEMU lanes and the board cycle the phases name are green.
- Security-relevant work has its `test_reject_*` and hardening markers
  (`docs/standards/SECURITY_STANDARDS.md`).
- Docs swept: CHANGELOG, affected docs, the RFC's status; `active_days` and, when the estimate
  missed, a `variance_note`.
