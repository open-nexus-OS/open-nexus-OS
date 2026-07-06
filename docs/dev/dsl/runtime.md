<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# App Runtime & Lifecycle

> STATUS: contract defined; implementation lands with TASK-0080D (app-host) and
> TASK-0076B (in-compositor mount). This page is the lifecycle SSOT and grows with them.

A compiled DSL program (`.nxir`) runs in one of two hosts — same runtime crate, same
semantics, different sinks:

1. **In-compositor mount** — the runtime embedded in the compositor/system-UI path,
   used for the system shell and the login greeter. The scene feeds
   `LayoutNode → LayoutEngine → SceneGraph → gfx` directly.
2. **App-host process** — one optimized runtime ELF; the spawner starts a **separate
   process per app**, which loads the app's `.nxir` from its installed bundle, renders
   into its own surface memory, and presents to the compositor over IPC.

An optional third tier (v0.3+): **AOT** — the same IR compiled ahead-of-time to a
per-app native binary. Behavior-identical to the interpreter by contract (golden-proven).

## Why apps start fast

- `.nxir` is a canonical binary IR with bounded, zero-parse reads — mounting means
  validating and indexing, not parsing or compiling.
- The app-host binary is already resident (it ships in the system image); a cold start
  is: spawn → fetch payload → validate → mount → first present.
- All arenas are allocated at mount; steady-state dispatch and paint perform **zero**
  heap allocation.
- Cold-start budget is measured per stage with markers and gated in CI (see `perf.md`).

## Launch pipeline

```text
build:   nx dsl build  →  payload.nxir (+ assets)  →  bundle (.nxb, signed manifest)
install: bundle manager verifies digest, registers app, serves payload
launch:  launcher → ability manager (capability check, fail-closed)
         → spawner (starts app-host process for payload kind "ui-program")
         → app-host: fetch payload → validate IR → mount → surface create
         → first present → visible
```

Authority stays in the platform services: the ability manager decides *whether* an app
launches; the session service decides *who* is logged in (the greeter is a DSL view over
it, never an authority); the compositor owns surface lifetimes.

## Lifecycle states

```text
Installed → Launching → Mounted → Visible ⇄ Hidden → Suspended → Terminated
```

- **Launching**: process spawned, payload fetched, IR validated.
- **Mounted**: stores initialized (persisted `@persist` fields restored), first scene
  built.
- **Visible/Hidden**: driven by the compositor (window state); hidden apps stop
  presenting but keep state.
- **Suspended**: `@persist` fields are written durably; the process may be reclaimed.
- **Terminated**: process exits; restart policy is the spawner's contract.

## Surfaces

Each app owns a surface backed by shared memory, presented to the compositor with
damage rectangles and a sequence/ack flow-control handshake. The transport contract
lives in its own ADR (cross-process surface transport). Input events are routed back
by surface id.

## Effects & services at runtime

Effects execute service calls through typed adapters with mandatory timeouts; results
re-enter the app as dispatched events. On the host, the same adapters run against
recorded transcripts — app logic is fully testable without the OS.

## Persistence tiers

1. Session state (default) — in-memory per app instance.
2. `@persist` store fields — typed snapshots via the state substrate on suspend.
3. Queryable data — through the query service contract only (see `db-queries.md`).

## Changelog

- **v1 (2026-07-06)** — lifecycle contract, two-host model, cold-start posture defined.
