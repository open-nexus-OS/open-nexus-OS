<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# App Runtime & Lifecycle

A compiled DSL program ([ir.md](ir.md)) runs in the **app-host**
(`source/services/app-host`): one runtime ELF, and execd starts a **separate process per
app**. The process loads the app's `.nxir` from its installed bundle, renders into its own
surface memory and presents to the compositor over IPC. Every DSL app goes this way — the
desktop shell, the greeter, settings, calculator, chat, stash and the on-screen keyboard.
The compositor (`windowd`) does not mount programs itself.

The same runtime crate (`userspace/dsl/runtime`, `no_std` + `alloc`) also runs on the host
under the harness below: `nx-dsl run` / `render`, the conformance suites and the golden
suites. One crate means one set of semantics; only the sinks differ. There is no
code-generation tier: the interpreter walks the IR directly.

## Why apps start fast

- `.nxir` is a canonical binary IR with bounded, zero-parse reads — mounting means
  validating and indexing, never parsing or compiling.
- The app-host binary ships in the system image; a cold start is: spawn → payload →
  validate → mount → first present.
- Per-frame work runs in generation arenas (below) and the base heap reuses freed blocks,
  so a long session does not grow the heap. The numbers and how to measure them are in
  [perf.md](perf.md).

## Launch pipeline

```text
build:   nx-dsl build  →  .nxir (+ locale packs)  →  bundle (manifest + payload)
install: bundlemgrd registers the app and serves its payload
launch:  launcher → abilitymgr (capability check, fail-closed)
         → execd (spawns app-host for payload kind "ui-program", grants the payload VMO)
         → app-host: read payload → validate IR → mount → create surface
         → first present → visible
```

An app-host started without a granted payload falls back to a program embedded at build
time (`APPHOST: payload source=…` names the path). Authority stays in the platform
services: abilitymgr decides *whether* an app launches and owns its lifecycle state; the
session service decides *who* is logged in (the greeter is a DSL view over it, never an
authority); windowd owns surface lifetimes. The manifest keys an app declares
(`payload_kind`, `caps`, `dependencies`, `restorable`) are in
[project-layout.md](project-layout.md).

## Lifecycle

The compositor states what is seen (focus, visible, occluded, closed); abilitymgr maps that
to a state and orders it; execd carries the order out on the process (RFC-0111):

```text
Created → Started → Foreground ⇄ Background → Suspended (frozen) → Stopped
```

- **Foreground / Background** follow the window: a hidden or occluded window is
  Background and the app stops presenting but keeps its state.
- **Suspended**: after a grace in Background the whole process is frozen. The wake is the
  window's restore; what the compositor sent meanwhile is delivered after the thaw.
- **Persisted state**: the `@persist` store fields of an app whose manifest says
  `restorable = true` are written as one snapshot (at most 2048 bytes) into the app's own
  statefs record when the window becomes hidden or closes, and read once at mount, before
  the initial effects. A snapshot over the limit is not written. An app without the
  `restorable` route sees no snapshot (`probe/persist.rs`, ADR-0086).
- **Stopped**: a user close runs the app's own exit first; a system kill (memory pressure,
  see below) leaves a ghost band the user can relaunch from.

## Surfaces

Each app owns a surface backed by shared memory, presented to windowd with damage
rectangles and a strictly sequenced present handshake (ADR-0042). Input is routed back by
surface id.

## Effects & services at runtime

Effects run service calls through typed adapters; the result re-enters the app as a
dispatched event. A service call has no client timeout: the exchange ends with the reply or
with the service's death ([services.md](services.md)). On the host the same adapters run
against recorded transcripts, so app logic is testable without the OS.

## Host harness

On the host the runtime mounts under a fixture environment: `FixtureEnv` (fixed `device.*`
values per profile), the identity locale (a key formats as its own text) and effect hosts
that are either `NoIo`, scripted (`Script` in `tests/dsl_conformance`) or a
`TranscriptHost` replaying a recorded transcript (`nx-dsl run --transcript`,
[cli.md](cli.md)).

`View::dispatch` returns the damage class: `Paint` — the existing layout geometry stays
valid (repaint only); `Layout` — re-layout; `None` — nothing visible changed. The class
comes from the dependencies the last emit recorded: every store field an emitted site reads
— in a binding, a branch condition, a modifier argument, a component prop — including reads
through list operations (`len`, `take`, `skip`, `map`/`filter` lambdas) and record
literals. A read the walk missed is a site that never repaints when only that field
changes (`userspace/dsl/runtime/tests/list_op_deps.rs` pins the list case).

The scene-golden suite (`tests/dsl_goldens`) renders retained scenes through the shared
painter; the conformance corpus (`tests/dsl_conformance`) pins `(state, event) → state'`
as the runtime's semantics contract ([testing.md](testing.md)).

## Presentation changes are reemits, never remounts

Theme, accent, shell profile, hour format, locale, timezone and keymap arrive as ONE
versioned presentation snapshot (`OP_SURFACE_SETTINGS`, RFC-0083). The host applies each
changed field by **re-emitting** the mounted view under the new tokens and device
environment: store state survives, and `@effect on Load` does not re-run. For app authors:

- **`if device.profile` arms re-select on a live profile switch** (the same mechanism as
  `device.sizeClass` on resize, and `device.keymap` on a keymap switch). Structure follows
  automatically ([profiles.md](profiles.md)).
- **`@effect on Load` fires once per mount**, not per profile or theme switch. A program
  that loads profile-dependent data declares the host event `ProfileEvent::Changed(tag)`
  and reloads there.
- Duplicate or reordered snapshots are free: every apply compares before acting; the
  snapshot generation is a dedupe token, not a protocol you see.

An event that carries an `@effect` and that nothing dispatches is a **root**: the runtime
fires it once at mount, in declaration order ([state.md](state.md)).

## Host-raised events

Some events are raised into the program by the host, not by a widget. The program declares
the event and its cases like any other; a program without the case does no work (no
dispatch, no relayout).

| Event | Cases | Raised when |
|---|---|---|
| `ProfileEvent` | `Changed(tag)` — the profile name as `Str` | the shell profile switched |
| `KeymapEvent` | `Changed(tag)` — the keymap tag as `Str` | the keymap switched |
| `ClockEvent` | `Tick(time, date)` — two formatted `Str`s | the wall-clock minute changed, or the locale or timezone did |
| `ImeStripEvent` | `Preedit(text)`, `Cands(…)` — eight candidate `Str`s | the composition strip changed (RFC-0075) |
| `MemoryPressure` | `Normal`, `Warn`, `Critical`, `Emergency` — no payload | the memory authority's level reached the window (RFC-0100 §3) |

`MemoryPressure` comes from the kernel through memd and the compositor on the window's own
event channel. At `Warn` a program drops what it can rebuild (a decoded image, a long
list's off-screen rows); at `Critical`, everything but what the user sees — the authority
may end a hidden process next. The host logs each arrival
(`apphost: mem pressure (level= handled=)`); `handled=1` means the program declared the
case and its dispatch changed the view.

## Memory over a session: the frame arena (ADR-0065)

An app-host rebuilds a frame on every structural interaction and throws it away. On a
service heap that never freed, all of it would leak. So the two frame phases run inside
**generation arenas**: the runtime's emission in one region, layout and text runs in
another, each region with two generations used alternately (the arena is
`FRAME_ARENA_BYTES` = 4 MiB, split evenly). Opening generation g+1 resets g−1 with a single
store. Two generations, because that is what the consumers need: the previous frame's boxes
are read by the next frame's row diff, the scene by its own layout and the next hit-test.
The regions are separate so a paint-only frame (emit, no layout) cannot reset the boxes it
still paints from.

**The rule that makes it safe, and that you must keep when touching `View::emit` or
`relayout_retained`:** nothing allocated inside a frame phase may outlive its generation.

- Every retained output — the scene, `deps`, `handlers`, `animations`, the layout result,
  the text runs — is a **fresh value each frame**. Never keep a buffer across frames with
  `clear()`: it is written in generation g+1 and reset underneath its owner in g+2, and the
  failure looks like a `Vec` with a capacity of `0xDEDE…`.
- Nothing durable is built inside the scope. The live-instance sweep over the stores and
  the text-focus record run after the emit guard has dropped.
- A reducer never runs inside a scope; store writes are on the ordinary heap by
  construction, because the scope opens in `View::emit`, after the reduce.
- A frame that **fails** is abandoned, never kept: the scope ends with `complete()` only
  when the frame is built, and a scope dropped on an early return discards its generation,
  so the frame on screen (the last one that completed) is never the one reset.

**The program is read per operation.** capnp's traversal budget belongs to a reader and
only counts down, so the runtime keeps the checked bytes (`Program`) and reads them through
a fresh reader for each emit, dispatch step or lookup (`Runtime::reader()`). Never keep a
reader across operations: one kept for an app's life ran dry after 28 menu toggles and
answered every later read `Malformed` (TASK-0344).

**Durable state frees.** What the arena cannot hold — store values, the dispatch queue,
anything that outlives a frame — lives on the service heap, which frees by size class
(every service, ADR-0065): eight classes from 16 to 2048 bytes, LIFO reuse, O(1), no
coalescing, so a session's working set is bounded by its peak per class. Blocks above the
largest class return to a bounded large list (first fit, neighbours merged). Each service's
heap is one 64 MiB region the kernel maps lazily (ADR-0077): a page costs the pool only once
touched.

This is enforced, not trusted: `tests/dsl_apps_conformance/tests/arena_invariant.rs` runs
the real apps for 64 frames under a **poisoning** two-generation allocator and aborts on any
stale read, and the OS build fills a reset generation with `0xDE` (feature
`frame-arena-poison` of `nexus-service-entry`) so a violation fails loudly in a boot.
The boot log reports `apphost: frame arena (layouts=… base=… peak=… of=… spill=…
abandoned=…)`; `spill=1` means a frame outgrew its generation and the leak is back,
`abandoned=` counts the frames that failed and were discarded.
`tests/dsl_apps_conformance/tests/frame_arena_budget.rs` holds each real app's per-frame
allocation under a recorded budget ([perf.md](perf.md)).

## Persistence tiers

1. Session state (default) — in-memory per app instance.
2. `@persist` store fields — one bounded snapshot per restorable app (see *Lifecycle*).
3. Queryable data — through the query service contract only ([db-queries.md](db-queries.md)).
