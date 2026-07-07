# ADR-0042: Cross-process surface transport — per-app VMO + present IPC + compositor blit (v1)

- Status: Accepted (implemented in TASK-0080D R1; deviations from the draft recorded below)
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

## Implementation deviations (R1, recorded)

1. **The APP allocates its surface VMO and moves a capability CLONE to windowd
   with `SURFACE_CREATE`** (the proven windowd→gpud attach pattern), instead of
   windowd allocating and returning the capability — IPC replies cannot carry a
   capability today. Quota/lifetime authority stays with windowd (it validates
   at create, closes its cap on destroy); ADR-0037's "windowd owns the memory"
   becomes "windowd owns admission + the compositor-side reference".
2. **New kernel syscall `VMO_READ` (47)** — the exact mirror of `VMO_WRITE`.
   Userspace has no VMO mapping path (the abi `vmo_map` was dead code, no kernel
   handler), so the damage-blit reads app pixels via a bounded explicit copy,
   symmetric with how windowd already writes the framebuffer.
3. **Acks return over windowd's shared response endpoint** (the same channel
   inputd holds RECV on) rather than a per-app channel — single-probe-app-safe
   for R1 because every other windowd client replies via moved reply caps.
   Per-app channels (minted at launch by abilitymgr) arrive with R3 multi-app.
4. **v1 blit granularity**: full-surface rows on present (bounded by the
   create-validated dims); the damage list bounds the queued SCREEN region.
   Blit-by-rect is the first recorded optimization.
5. **Wire envelope**: the frames ride windowd's server endpoint, which speaks
   the `[b'I', b'N', ver, op]` family — the codecs live in
   `nexus-display-proto::client_surface` (the display SSOT), ops 8–10 in the
   shared op space (collision pinned by test).

## Deviation 6 (2026-07-07): dedicated per-app event channel replaces shared window_rsp delivery

The R1 shortcut (deviation 3: acks over the shared `window_rsp` endpoint)
was UNSOUND for input: `inputd` holds a RECV on the same endpoint and drains
it continuously for its own acks, so an `OP_SURFACE_INPUT` frame could be
consumed by ANY receiver — live taps never reached the app (the R3 "buttons
do nothing" failure; the `WINDOWD: surface input routed` marker was also
hollow, printing before delivery was known). Now: nexus-init mints a
dedicated endpoint pair in the execd arm (`init: execd app-event slots`),
execd moves a SEND clone to windowd (`OP_SURFACE_EVENTS`, cap-move — sent on
the same request queue BEFORE the child resumes, so windowd attaches the
channel before any surface op arrives) and grants RECV to the child
(slot 8). windowd delivers ALL app-bound frames (input events + surface
acks) on this channel; the shared endpoint remains only as a marked fallback
for old wiring. Markers are honest: `routed` prints only on a delivered
send, and every silent path in the app-host event loop is bounded-marked
(recv errors, non-input frames, tap misses).
