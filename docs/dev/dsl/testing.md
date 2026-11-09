<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Testing

The DSL is **host-first**: most correctness is proven by host tests that run the real
compiler and the real runtime crate; QEMU is a bounded smoke layer with deterministic
markers ([perf.md](perf.md) names the ones the runtime emits).

```bash
just test-host                       # the whole workspace except the kernel
env RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"' \
  cargo +nightly-2025-01-15 test -q -p dsl_conformance      # one suite
```

Goldens (IR bytes, scenes, pixels) are regenerated with `UPDATE_GOLDENS=1` on the suite's
command line; review the diff before keeping it.

## Where the proofs live

Host suites are split by the KIND of proof, and a feature gets an acceptance FILE inside the
crate that matches its kind — not a crate of its own:

| crate | what it owns |
| --- | --- |
| `userspace/dsl/{core,ir,runtime,cli}/tests` and in-file unit tests | the lexer, parser and formatter (`fmt` is a fixpoint), token vocabularies, IR validation, runtime behaviours (list dependencies, text editing, number formatting, …), the CLI verbs end to end |
| `tests/dsl_conformance` | language + runtime semantics, with scripted services (`Harness`/`Script`): the accept/reject corpus, async recipes, transcript replay, queries, effect cancellation, and the docs gate below |
| `tests/dsl_v0_1a_host` | determinism, IR goldens, loader validation, and the slots acceptance suite (`slots.rs`) |
| `tests/dsl_goldens` | emitted scenes and rendered pixels, including the profile matrix and the locale packs |
| `tests/dsl_apps_conformance` | the REAL compiled apps — hit targets, panels, presentation, the frame-arena invariant and budgets ([runtime.md](runtime.md)) |

## What to test

- parse/format idempotence and lowering determinism (same source, same bytes);
- diagnostics: a rule that rejects input is proven by a `test_reject_*` test that names the
  code it expects;
- `(state, event) → state'` for reducers and effect plans, with a scripted or transcript
  host standing in for services ([services.md](services.md), `nx-dsl run --transcript`);
- damage classes: a change that should only repaint must not re-lay out;
- the real apps compile, mount and answer their core interaction against a fake service host;
- a restructure keeps the scene: dump the laid-out boxes, the texts and the handler payloads
  before and after (with real data loaded — an empty list hides a broken template) and diff
  them; only a handler's trigger symbol id may move, as new names shift the symbol table.

Every app also gets two generic proofs for free: the structure gate (`app_structure.rs`: thin
pages, feature folders, README sections, a test per app) and the smoke run (`app_smoke.rs`:
every route mounts and every event case reduces with neutral payloads while every service
refuses — `Runtime::case_probes`/`route_probes`, phone and desktop).

## The docs gate

`docs/dev/dsl/` is outside the docs ledger gate, so two tests in `tests/dsl_conformance` keep
it honest:

- `docs_examples`: every fenced block tagged `nx` (a whole program) or `nx-view` (a view
  fragment, checked inside a page) compiles; every widget has a heading of its own name in
  [widgets.md](widgets.md) and every live modifier appears in [modifiers.md](modifiers.md).
  Any other tag (`text`, `ebnf`, `bash`, …) is not checked, so pseudo-code says so with its tag.
- `doc_proof_map`: every `crate::file::test` reference in the table below resolves to a real
  test function. Rename a test and the doc fails until it follows.

## Guarantee → proof

| guarantee | proven by |
| --- | --- |
| reducers update state deterministically; checked math errors | `dsl_conformance::corpus::reducers_update_state_deterministically`, `equal_writes_do_not_mark_changes_and_checked_math_errors` |
| build twice ⇒ byte-identical `.nxir`; the IR golden holds | `dsl_v0_1a_host::determinism::build_is_byte_deterministic`, `proof_surface_matches_the_ir_golden` |
| the loader rejects a wrong major and corrupt bytes | `dsl_v0_1a_host::determinism::loader_rejects_wrong_major_and_corruption` |
| per-instance keyed `$state` across a reorder | `dsl_conformance::corpus::keyed_instances_keep_their_own_state_across_a_reorder` |
| the two-way bind follows the widget catalog | `dsl_conformance::corpus::the_bind_rule_follows_the_registry_not_a_list`, `every_bound_control_says_how_its_value_is_produced` |
| every bindable control's handler is live | `dsl_conformance::corpus::every_bindable_control_gets_a_handler_whose_trigger_resolves` |
| a bound `Slider` writes the track fraction | `dsl_goldens::scenes::a_bound_slider_writes_the_fraction_of_its_track`, and against the real shell in `dsl_apps_conformance::shell_control_center` |
| both `Ok` and `Err` are mandatory (`NX0407`) | `dsl_conformance::corpus::test_reject_a_service_result_whose_paths_are_not_both_handled` |
| effects run FIFO; a runaway dispatch loop is cut | `dsl_conformance::corpus::effect_scheduling_is_fifo_and_declaration_ordered`, `cascade_budget_stops_runaway_dispatch_loops` |
| the async recipes + the no-automatic-retry limit | `dsl_conformance::async_recipes` |
| latest-wins effect cancellation | `dsl_conformance::corpus::stale_effect_followups_are_cancelled_when_the_trigger_refires` |
| a transcript replay miss is an error, never a default | `dsl_conformance::transcript::a_replay_miss_surfaces_as_the_miss_code_never_a_default` |
| platform overrides wrap the base page per profile | `dsl_conformance::corpus::platform_overrides_wrap_the_base_page_per_profile` |
| `@persist` fields restore across mounts | `dsl_conformance::corpus::persist_snapshot_restores_marked_fields_across_mounts` |
| env/profile variants | `dsl_goldens::scenes::profile_matrix_goldens_are_stable_and_distinct` |
| nothing a frame allocates outlives its generation | `dsl_apps_conformance::arena_invariant::nothing_emission_allocates_outlives_its_generation` |
| every app's routes mount and every event case reduces and runs its effects with neutral payloads | `dsl_apps_conformance::app_smoke::every_route_mounts_and_every_event_reduces` |
| every app follows the project layout, or is in the shrinking baseline; a README carries its sections | `dsl_apps_conformance::app_structure::every_app_follows_the_project_layout_or_is_in_the_shrinking_baseline`, `test_reject_a_readme_without_its_sections` |
