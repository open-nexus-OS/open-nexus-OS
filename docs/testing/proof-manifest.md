---
schema: 1
title: "Proof manifest"
role: reference
subsystem: devx
sources: [source/apps/selftest-client/proof-manifest, source/libs/nexus-proof-manifest, source/apps/selftest-client/build.rs, source/apps/selftest-client/src/os_lite/profile.rs, tools/qmp_scenario.py]
contracts: [RFC-0038, RFC-0014, ADR-0027, ADR-0089]
verified: 2026-11-13
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Proof manifest

> **Purpose** — the schema of the proof-manifest directory: phases, markers, profiles and lane scenarios, and how to extend each.
> **Owns** — `source/apps/selftest-client/proof-manifest/` (data), `source/libs/nexus-proof-manifest/` (parser + CLI)
> **Contracts** — [RFC-0038](../rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md) (manifest as the one source), [RFC-0014](../rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md) (the phase list), [ADR-0027](../adr/0027-selftest-client-two-axis-architecture.md) (selftest-client structure), [ADR-0089](../adr/0089-lane-choreography-is-declared-the-guest-opens-the-input-window-every-console-line-is-whole.md) (lane choreography as data)
> **Invariants** — the schema is closed: an unknown key is a parse error. A profile owns its lane's topology; a contradicting caller env is a hard error. The parser is host-only (never in the OS graph).
> **Read next** — [os-markers.md](os-markers.md), [evidence-bundle.md](evidence-bundle.md)

## Layout

```
source/apps/selftest-client/proof-manifest/
  manifest.toml        [meta] + [include] (schema v2 root)
  phases.toml          [phase.<name>] order = 1..12
  markers/<phase>.toml [marker."<literal>"] — one file per phase, plus board.toml, ui.toml, usb.toml
  profiles/harness.toml  [profile.<name>] runner/extends/env — lanes
  profiles/runtime.toml  [profile.<name>] runtime_only/phases — in-guest phase subsets
  scenarios/<name>.toml  injector choreography per lane; scenarios/targets.toml points + words
```

## Schema v2

```toml
# manifest.toml
[meta]
schema_version = "2"
default_profile = "full"        # must resolve to a [profile.X]

[include]
phases   = "phases.toml"
markers  = "markers/*.toml"     # globs resolve lexicographically
profiles = "profiles/*.toml"
```

| Table | Keys |
|---|---|
| `[phase.<name>]` | `order` (unique, 1-based) |
| `[marker."<literal>"]` | `phase` (required, declared), `proves`, `introduced_in`, `emit_when`, `emit_when_not`, `forbidden_when` (each `{ profile = "<name>" }`, must name a declared profile) |
| `[profile.<name>]` harness | `runner` (`scripts/qemu-test.sh`, `tools/os2vm.sh`, `scripts/board-test.sh`), `extends`, `env = { … }` |
| `[profile.<name>]` runtime | `runtime_only = true`, `phases = [ … ]` (no `runner`) |

An included file may hold only its own category. Duplicates across files, an `extends` cycle, an
unknown parent and a `runner` on a runtime profile are rejected. Every reject is one
`ParseError` variant in `source/libs/nexus-proof-manifest/src/error.rs`, each with a test under
`source/libs/nexus-proof-manifest/tests/`. A schema change bumps `schema_version` and the parser together.

## Phases

The 12 phases of RFC-0014 in `phases.toml` map one to one onto
`source/apps/selftest-client/src/os_lite/phases/<name>.rs`: `bringup`, `ipc_kernel`, `mmio`,
`routing`, `ota`, `policy`, `exec`, `logd`, `vfs`, `net`, `remote`, `end`. A marker's phase is
the phase it is declared under, not its position in the UART. Adding a phase: update RFC-0014,
add `[phase.<name>]`, add `os_lite/phases/<name>.rs` (registered in `phases/mod.rs`), extend
`PhaseId` in `os_lite/profile.rs` and the `run_or_skip!` chain in `os_lite/dispatch.rs`.

## Profiles

Harness profiles shape the outside of a run (machine, devices, required proofs); runtime profiles
shape the inside (which phases the guest runs). Harness profiles chain with `extends`; children
shadow parent keys. `full` is the root (it carries the lane memory envelope), `headless` the
common base. A runtime profile is selected at boot through `NEXUS_SELFTEST_PROFILE`
(`ci-os-runtime-*`); skipped phases print `dbg: phase <name> skipped`.

**The profile owns the topology.** `pm_apply_profile_env` in `scripts/qemu-test.sh` exports the
resolved `env`; a caller value that contradicts a declared key (`SMP`, `QEMU_NO_ICOUNT`,
`REQUIRE_*`, …) exits 1 before the build and names the key. `NEXUS_PROFILE_ENV_OVERRIDE=1` keeps the
caller's value with a warning, for local one-offs only. `just arch-gate` Rule 6 forbids
`REQUIRE_*` literals in `test-*`/`ci-*` recipe bodies. Host tests keep the boot lanes honest
without booting (`cargo test -p nexus-proof-manifest --test profiles`):
`accept_boot_lane_topology_is_declared` pins `smp`/`smp1`, and
`reject_require_smp_without_two_harts_on_disk` rejects `REQUIRE_SMP=1` with fewer than two harts.

## Scenarios (ADR-0089)

A lane that injects input names a scenario in its env (`QEMU_INPUT_SCENARIO`); one runner,
`tools/qmp_scenario.py` over `tools/qmp_input.py`, executes it.

```toml
[scenario]
profile = "lowmem"
start_after = "SELFTEST: input window open"   # the guest opens the input window

[[step]]
name = "the keyboard reaches the greeter"
do = { key = "a" }                            # login | click | move | chord | key | type | drag | wait
expect = ["inputd: live keyboard route on"]   # new lines after the action, any order
timeout_s = 10
retry = 2
```

| Key | Meaning |
|---|---|
| `expect` | markers that must appear as new lines after the action, in any order |
| `then` | markers that must follow the last `expect` line |
| `timeout_s` | bound for `expect` + `then` |
| `retry`, `retry_with` | declared retries; otherwise a missing acknowledgement ends the run: `[scenario] FAIL step=… missing=…` (also in `hypothesis.json`) |
| `acked_by` | an action that prints nothing (typing — privacy rule) names the later step that covers it |
| `settle_s` | a declared pause with no event to wait for (bounded) |

A `click` is also acknowledged by app-host's tap readback; a miss (`input tap miss at`) is retried.
A launch step ends on the app's first frame, app-host's launch line (`apphost: ux launch `): a
window has one surface, laid out for its store when it is created. A tile and its return
re-create the surface, so their steps still `expect` `apphost: resized` and `then` the new
surface's first present (`WINDOWD: surface presented id=`).
Points and typed words come from `scenarios/targets.toml` (`[targets]`, `[words]`), the same file
the shell conformance tests in `tests/dsl_apps_conformance/tests/` read. Every step logs its
duration (`scenario step ok`). The contract is checked on the host:
`cargo test -p nexus-proof-manifest --test scenarios` or
`nexus-proof-manifest scenario --scenario=all` (every literal declared, every point known, every
action acknowledged, every bound bounded).

## CLI

`nexus-proof-manifest <cmd> [--manifest=<path>] [--profile=<p>] [--format=lines|json|shell]`
(default manifest: `source/apps/selftest-client/proof-manifest/manifest.toml`):

| Command | Output |
|---|---|
| `list-markers --profile=<p>` | the expected ladder of a profile |
| `list-forbidden --profile=<p>` | markers the profile forbids |
| `list-env --profile=<p>` | the resolved env (what the harness exports) |
| `list-phases` | phases in order |
| `verify` | parse only |
| `verify-uart --profile=<p> --uart=<log>` | deny-by-default judgement of a UART log |
| `scenario --scenario=<name>\|all` | a validated scenario as JSON, or check all |

## Extending

- **Marker** — [os-markers.md](os-markers.md) § Declaring a marker.
- **Harness profile** — add `[profile.<name>]` with `runner`, `extends`, `env`; add a
  `ci-os-<name>` recipe that runs `just test-os <name>` (no `REQUIRE_*` in the recipe); check with
  `list-env` and `list-markers`.
- **Runtime profile** — add it to `profiles/runtime.toml`, a `Profile` variant in
  `source/apps/selftest-client/src/os_lite/profile.rs`, and a `ci-os-runtime-<name>` recipe.
- **Scenario** — add `scenarios/<name>.toml`, name it in the profile's `QEMU_INPUT_SCENARIO`,
  run the scenario check.
