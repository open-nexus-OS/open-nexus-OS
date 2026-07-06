# ADR-0042: Cross-process surface transport — per-app VMO + present IPC + compositor blit (v1)

- Status: Proposed (drafted for the DSL app-runtime track; implemented in TASK-0080D phase 6a)
- Created: 2026-07-06
- Builds on: ADR-0037 (per-app surface VMO + lifecycle residency), ADR-0038 (display wire
  SSOT = `nexus-display-proto`; capnp display schemas are descriptive only), ADR-0036
  (authority split), RFC-0065 (app lifecycle).
- Realizes: the missing transport for "apps as real processes" — today `windowd::create_surface`
  is only callable in-process (the launcher links windowd; chat/search are baked into the atlas).

## Context

ADR-0037 already decides *ownership and residency*: each app owns its own surface VMO,
windowd owns the VMO, abilitymgr owns when it exists, each app composites as its own
`Layer`. What it does not define is the **transport**: how a separate app *process*
obtains access to its surface and gets pixels to the screen. That gap blocks the DSL
app-host (one runtime ELF, spawned per app by execd), and any future native app process.

Physical constraint: the present path scans out of **one framebuffer VMO** — gpud
samples layers by absolute row/column inside that single VMO (shared-atlas model,
`windowd/src/atlas.rs`). A separate per-app VMO is not directly addressable by gpud
today.

## Decision (v1)

**Per-app surface VMO + present IPC + windowd damage-blit into the atlas.**

1. **Wire**: extend `source/libs/nexus-display-proto` (the display wire SSOT per
   ADR-0038) with client-surface control frames:
   - `SURFACE_CREATE { width, height, format }` → windowd allocates the surface VMO
     (it owns lifetime/quota per ADR-0037), registers the app layer, and returns
     `{ surface_id }` + the VMO capability via `cap_transfer`.
   - `SURFACE_PRESENT { surface_id, seq, damage[≤N] }` → windowd blits the damaged
     rects from the app's VMO into the atlas region backing that app's `Layer`, marks
     the scene dirty, and acks with `{ seq }`.
   - `SURFACE_DESTROY { surface_id }` → existing `destroy_surface` path (ADR-0037).
   The capnp surface schema stays **descriptive only** (ADR-0038).
2. **Flow control**: strictly sequenced `seq`/ack — an app never has more than one
   un-acked present in flight. No tearing risk (windowd blits from a quiescent buffer);
   double-buffered app VMOs are deferred until profiling shows a need.
3. **Input routing**: windowd routes pointer/keyboard events to the focused surface's
   owning connection by `surface_id` (reuses the existing hit-testing/interaction
   model; windowd stays the input authority).
4. **Authority unchanged**: abilitymgr decides launch/residency; execd spawns the app
   process and transfers the windowd client capability; windowd owns surface memory,
   composition, and input routing. Apps get pixels and events — nothing else
   (information hiding at the process boundary).

## Why blit (and not zero-copy) in v1

- The blit touches only **damage rects** (bounded, typically small for reactive UIs);
  it is boring, debuggable, and requires no gpud changes.
- Alternatives rejected for v1:
  - **gpud multi-VMO sampling** — gpud maps every app VMO; churns the proven
    gpud/windowd wire + VA-slot management (gpud VA slots are never reused) for an
    optimization we cannot yet measure.
  - **Atlas-slot lease** — the app renders directly into a leased atlas sub-rect
    (row-pitch handshake); zero-copy, but hands hostile processes a window into the
    shared framebuffer VMO unless sub-VMO mapping/protection exists first.
- **Recorded optimization path**: atlas-slot lease (or sub-VMO grants) once profiling
  shows the blit on the frame budget; the wire contract above does not change — only
  where the app's pixels land.

## Consequences

- Positive: real app processes become possible with a small, additive wire extension;
  per-app isolation (own VMO, own layer, own process); the DSL app-host, AOT apps, and
  future native apps all use the same transport.
- Cost: one CPU copy per damaged rect per present (bounded by damage cap `N` and
  surface size); acceptable for v1 and measured by the perf phase
  (`PERF: cold_start_ms`, present latency markers).
- Bounded: `MAX_APP_SURFACES` (ADR-0037) caps resident surfaces; damage list capped;
  one in-flight present per surface.
- Sequencing: probe first — an app-host skeleton that fills its VMO with a solid color
  and presents, before any DSL involvement (TASK-0080D phase R1).
