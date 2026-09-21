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

There is no third tier: an ahead-of-time codegen tier was retired by decision on
2026-09-09 (TASK-0079) — the interpreter in the app-host process is the shipped
execution path, and its scale contract (long sessions, large data) is TASK-0077C.

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

## Host harness (shipped, TASK-0076)

On the host the same runtime mounts under a fixture environment: `FixtureEnv`
(fixed `device.*` values), the identity locale (a key formats as its own text —
the pseudo-locale), and scripted/`NoIo` effect hosts. `View::dispatch` returns
the damage class — `Paint` means the existing layout geometry stays valid
(repaint only), `Layout` means re-layout, `None` means nothing visible changed.
The scene-golden suite (`tests/dsl_goldens`) renders retained scenes through
the shared BGRA painter; the conformance corpus (`tests/dsl_conformance`)
pins `(state, event) → state'` semantics as the runtime's semantics contract.

## Presentation changes are reemits, never remounts (RFC-0083)

Theme, accent, shell profile, locale, timezone and keymap arrive as ONE
versioned presentation snapshot (`OP_SURFACE_SETTINGS`). The host applies each
changed field by **re-emitting** the mounted view under the new tokens/device
env — store state survives by construction, and `@effect on Load` does NOT
re-run. Consequences for app authors:

- **`if device.profile` arms re-select on a live profile switch** (the same
  mechanism as `device.sizeClass` on resize). Structure follows automatically.
- **`@effect on Load` fires once per mount, not per profile/theme switch.**
  A program that loads profile-dependent data declares the runtime event
  `ProfileEvent::Changed(tag)` and reloads there — the `KeymapEvent::Changed`
  precedent (ime-ui reloads its OSK rows the same way).
- Duplicate or reordered snapshots are free: every apply compares before
  acting; the snapshot generation is a dedupe token, not a protocol you see.

## Memory over a session: the frame arena (ADR-0065)

An app-host rebuilds a frame on every structural interaction and throws it away —
measured against the real desktop-shell: 226 KiB per layout call, ~100 KiB per
structural emit, and **0 B of live drift** over 100 dispatches. On a service heap
that never frees, all of it leaks, and a 16 MiB heap buys 50–85 interactions.

So the two frame phases run inside **generation arenas**: the runtime's emission
in one region, layout + text runs in another, each with two generations used
alternately. Opening generation g+1 resets g-1 with a single store. Two, because
that is exactly what the consumers need — the previous frame's boxes are read by
the next frame's row diff, the scene by its own layout and the next hit-test.
The regions are separate so a paint-only frame (emit, no layout) cannot reset
the boxes it still paints from.

**The rule that makes it safe, and that you must keep when touching `View::emit`
or `relayout_retained`:** nothing allocated inside a frame phase may outlive its
generation.

- Every retained output — the scene, `deps`, `handlers`, `animations`, the
  layout result, the text runs — is a **fresh value each frame**. Never keep a
  buffer across frames with `clear()`: it is written in generation g+1 and reset
  underneath its owner in g+2, and the failure looks like a `Vec` with a
  capacity of `0xDEDE…`.
- Nothing durable is built inside the scope. The live-instance sweep over the
  stores and the text-focus record run after the emit guard has dropped.
- A reducer never runs inside a scope; store writes are on the ordinary heap by
  construction, because the scope opens in `View::emit`, after the reduce.

**And durable state frees.** What the arena cannot hold — store values, the
dispatch queue, anything that outlives a frame — still churned on a heap that
never freed: 432 B per layout on a real boot, a ceiling of ~39 000 interactions.
So the app-host's base heap frees by size class (`small-object-free-list`,
ADR-0065): eight classes from 16 to 2048 bytes, LIFO reuse, O(1), no coalescing
— a session's working set is bounded by its peak per class, and the heap is
back on the 4 MiB floor the 8 and 16 MiB raises had left.

This is enforced, not trusted: `tests/dsl_apps_conformance/tests/arena_invariant.rs`
runs the real apps for 64 frames under a POISONING two-generation allocator and
aborts on any stale read, and the OS build fills a reset generation with `0xDE`
(`frame-arena-poison`) so a violation fails loudly in a boot instead of corrupting
silently. The boot log reports `apphost: frame arena (layouts=… base=… peak=… of=…
spill=…)`; `spill=1` means a frame outgrew its generation and the leak is back.

## Persistence tiers

1. Session state (default) — in-memory per app instance.
2. `@persist` store fields — typed snapshots via the state substrate on suspend.
3. Queryable data — through the query service contract only (see `db-queries.md`).

## Changelog

- **v2 (2026-07-27, RFC-0083)** — presentation snapshot semantics: settings
  changes are reemits (store survives, no `@effect on Load` re-run);
  `ProfileEvent::Changed` is the profile-data reload hook.
- **v1 (2026-07-06)** — lifecycle contract, two-host model, cold-start posture defined.
