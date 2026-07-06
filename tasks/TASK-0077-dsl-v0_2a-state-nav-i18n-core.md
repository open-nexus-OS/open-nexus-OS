---
title: TASK-0077 DSL v0.2a: stores/effects scheduling + routes/navigation + i18n + device-env/profile overrides (host)
status: Draft
owner: @ui @runtime
created: 2025-12-23
updated: 2026-07-06
depends-on:
  - tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
follow-up-tasks:
  - tasks/TASK-0077B-dsl-v0_2a-devx-ergonomics-local-state-env-async-recipes.md
  - tasks/TASK-0077C-dsl-v0_2c-pro-primitives-nativewidget-virtual-tables-timelines.md
  - tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Language reference: docs/dev/dsl/{grammar,state,navigation,i18n,profiles}.md
  - Device-env SSOT (EXISTS, ADR-0035): source/services/systemui/manifests/{products,profiles,shells}/
    + source/services/systemui/src/{registry,product,profile,shell}.rs
  - Data formats rubric: docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context (updated 2026-07-06)

v0.1 gives us a mounted, reactive page. v0.2a adds the app mechanics: multi-store
programs with deterministic effect scheduling, routes/navigation, i18n, and the
responsive story — a **default UI plus per-device overrides**, driven by a read-only
`device.*` environment.

**IST correction:** the profile/shell registry this task once proposed now EXISTS as
the SystemUI shell-config registry (ADR-0035): `product.toml` selects profile+shell+
theme; `profile.toml` carries `[input]` flags and `[display_defaults]`
(orientation/dpi_class/size_class); `shell.toml` carries `dsl_root` +
`supported_profiles` + `[first_frame]`. The DSL device environment is **derived from
that registry** — this task must not invent a second one. App-side profile overrides
(`ui/platform/<profile>/…`) remain an app-project concept resolved at build time.

**Syntax note:** the canonical conditional is `if/else` (grammar.md v1); the former
`@when/@else` form is gone. `match` is exhaustive.

## Device environment contract (read-only)

- `device.profile` — validated profile id (`phone|tablet|desktop|tv|auto|foldable|convertible` baseline; forks may add ids via manifests)
- `device.posture` — `{flat, half_fold, tent, book}` (only when foldable)
- `device.orientation` — `{portrait, landscape}`
- `device.shellMode` — validated shell id (explicit operating mode, never a hardware proxy)
- `device.sizeClass` — `{compact, regular, wide}`
- `device.dpiClass` — `{low, normal, high}`
- `device.input` — flags `{touch, mouse, kbd, remote, rotary}`

Values resolve from the systemui registry types on OS; host tests inject fixtures.
Unknown ids / incompatible profile-shell pairings reject deterministically.

## Profile overrides (build-time, deterministic)

- `ui/platform/<profile>/pages/<Page>.nx` overrides `ui/pages/<Page>.nx` (same for
  components); fixed precedence, no filesystem-order dependence; conflicts are lint
  errors; the merge happens at `nx dsl build` with **provenance recorded in the IR**.
- Inline branching `if device.profile == … { } else { }` lowers to IR `DeviceCond`
  branches; missing final `else` on profile branches = Warning (`--deny-warn`).
- Overrides stay profile-keyed; orientation/shell-mode differences use inline branches.
- Apps branch on responsive layout + `device.profile` first; only shell-owned surfaces
  additionally branch on `device.shellMode`.

## Goal

1. **Grammar/IR/lowering** (extend `userspace/dsl/{core,ir}`): multi-store programs,
   `Routes { "/" -> Home; "/detail/:id" -> Detail(id: UserId); }` + navigate handlers,
   `@t("key", args…)` collection, `DeviceCond` dedup, override-merge provenance,
   `@persist` field flag (restore/persist semantics land with the OS substrate).
2. **Runtime** (extend `userspace/dsl/runtime`, module layout pinned):
   - `store.rs`/`effects.rs`: dispatch → reduce (pure) → commit → scheduled effects
     (bounded queue, deterministic order); effect steps abstract until TASK-0078;
   - `nav.rs`: typed params, push/replace/back, bounded history, subtree mount/unmount
     through the retained tree (state of kept-alive routes per contract);
   - `i18n.rs`: compiled locale catalogs (authoring JSON → compiled binary per
     ADR-0021), `@t` lookup, locale switch = paint-class invalidation of bound sites,
     fallback chain, pseudo-locale for tests;
   - `env.rs`: DeviceEnv trait + fixture impl (host) + registry-derived impl (OS, used
     by 0076B mount and later phases).
3. **Virtualized collection groundwork**: the keyed `List` template gains a windowed
   mode contract (full virtualization behavior in TASK-0077C consumer wave).

## Non-Goals

- Real `svc.*` IPC (TASK-0078); QuerySpec (TASK-0078B); local `$state` sugar +
  async recipes (TASK-0077B); OS wiring beyond keeping 0076B green. Kernel changes.

## Constraints / invariants (hard requirements)

- Deterministic update ordering; effects never run inside reducers; bounded queues
  (events/effects/history caps from IR budgets).
- Zero-alloc steady state preserved (locale switch and route change may allocate at
  transition, never per-frame).
- riscv no_std build stays green for core/ir/runtime.
- No `unwrap/expect`; no godfiles; no company/product names.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_2a_host/`:

- reducer purity: `svc.*` in a reducer = stable diagnostic (fixture);
- store runtime: multi-store dispatch deterministic; effect scheduling order
  deterministic (fixture with competing effects);
- navigation: push/params/replace/back fixtures; history cap enforced; deep-link
  parse round-trips; back restores kept-alive state per contract;
- i18n: required keys extracted; missing key = error; locale switch updates all bound
  sites (paint-only — layout untouched unless metrics change); fallback chain +
  pseudo-locale goldens;
- device env: profile fixture matrix goldens — at minimum `phone±portrait/landscape`,
  `tablet±portrait/landscape`, `desktop`, `convertible+desktop-shell`,
  `convertible+tablet-shell`; file-override fixture proves precedence + provenance;
- conformance corpus extended (nav + i18n + multi-store cases).

### Docs — required (reference grade)

- `docs/dev/dsl/{navigation,i18n,profiles}.md` to full reference chapters (profiles.md
  documents the registry derivation — SSOT = systemui manifests, ADR-0035);
- `state.md` updated for effect scheduling semantics.

## Touched paths (allowlist)

- `userspace/dsl/{core,ir,runtime}/` (extend), `userspace/dsl/cli/` (build override
  merge, i18n extract groundwork)
- `tests/dsl_v0_2a_host/` (new), `tests/dsl_conformance/` (extend)
- `docs/dev/dsl/{state,navigation,i18n,profiles}.md`

## Plan (small PRs)

1. grammar/IR: routes + i18n keys + DeviceCond + override merge (+ fmt)
2. store/effect scheduling runtime + purity/queue proofs
3. nav runtime + fixtures
4. i18n compile + runtime + pseudo-locale
5. env.rs + profile matrix goldens + docs

---

## STATUS / PROGRESS LEDGER (updated 2026-07-06)

### ✅ DONE (first increment)

- **Navigation runtime** (`userspace/dsl/runtime/src/nav.rs`): route table from the IR
  (paths already land in `.nxir` since v1.0), segment matching with `:param`
  placeholders, **typed param parsing** (Int-typed params reject non-numeric text —
  the route simply doesn't match), bounded history (`MAX_HISTORY=32`, `RtError::Budget`),
  push/replace/back with a never-empty root. `View` now renders `nav.current().page`
  (entry = the `/` route, falling back to `entryPage`); `View::navigate/navigate_back`
  re-emit with `Damage::Layout`. Conformance test: push/replace/back/typed-params/budget.
- **Tooling prerequisite shipped** (user request, 2026-07-06): DSL chain tests with hop
  markers — `tools/nx/src/chain/contract/dsl_mount.rs` (`DslMountContract`: healthy /
  pool-starved / invalid-program modes) + `tools/nx/tests/chain_dsl_mount.rs` (4 tests:
  happy chain, value-carrying atlas denial, fail-closed validation, reserve contract).
  Boot regressions in the mount chain now name the exact failing hop host-side.

### ⬜ OPEN (this task's remainder — see Goal)

- Route-param **binding into page views** (needs lowering support: route params as the
  page's param slice; pages currently take no params).
- `navigate` as a DSL handler action (grammar/IR/lowering — today navigation is a host
  API on `View`).
- Multi-store programs, effect-scheduling determinism fixtures, i18n compiled catalogs +
  locale switch, `device.*` from the systemui registry + profile-matrix goldens,
  `ui/platform/<profile>/` build-time overrides, kept-alive route state contract,
  windowed-List consumer contract (0077C core).
