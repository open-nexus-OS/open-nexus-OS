<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Testing

The DSL is **host-first**:

- most correctness is proven via host tests,
- QEMU is a bounded smoke layer with deterministic markers.

## Snapshot testing (v0.1b+)

```bash
nx dsl snapshot <appdir> --route / --profile desktop --locale en-US
```

Conventions:

- inputs/fixtures live under `ui/tests/fixtures/`
- goldens live under `ui/tests/goldens/`
- generated artifacts live under `target/nxir/` and `target/nxir/snapshots/`

## What to test

- parse/format idempotence
- lowering determinism and diagnostics
- reducer purity violations are rejected

## Where the proofs live

Host suites are split by the KIND of proof, and a feature gets an acceptance
FILE inside the crate that matches its kind — not a crate of its own:

| crate | what it owns |
| --- | --- |
| `tests/dsl_conformance` | language + runtime semantics, with scripted services (`Harness`/`Script`) — the accept/reject corpus, async recipes, effect cancellation |
| `tests/dsl_v0_1a_host` | determinism, IR goldens, loader validation, and the slots acceptance suite (`slots.rs`) |
| `tests/dsl_goldens` | emitted scenes and rendered pixels, including the profile matrix |
| `tests/dsl_apps_conformance` | the REAL compiled apps — hit targets, panels, presentation |

Guarantee → proof, for the v0.2a DevX surface:

| guarantee | proven by |
| --- | --- |
| per-instance keyed `$state` across a reorder | `dsl_conformance::corpus::keyed_instances_keep_their_own_state_across_a_reorder` |
| the two-way bind follows the widget catalog | `dsl_conformance::corpus::the_bind_rule_follows_the_registry_not_a_list`, `every_bound_control_says_how_its_value_is_produced` |
| every bindable control's handler is live | `dsl_conformance::corpus::every_bindable_control_gets_a_handler_whose_trigger_resolves` |
| a bound `Slider` writes the track fraction | `dsl_goldens::scenes::a_bound_slider_writes_the_fraction_of_its_track`, and against the real shell in `dsl_apps_conformance::shell_control_center` |
| both `Ok` and `Err` are mandatory (`NX0407`) | `dsl_conformance::corpus::test_reject_a_service_result_whose_paths_are_not_both_handled` |
| the async recipes + the no-automatic-retry limit | `dsl_conformance::async_recipes` |
| latest-wins effect cancellation | `dsl_conformance::corpus::stale_effect_followups_are_cancelled_when_the_trigger_refires` |
| env/profile variants | `dsl_goldens::scenes::profile_matrix_goldens_are_stable_and_distinct` |
