# ADR-0037: Each app owns its surface VMO, lazily allocated when active and freed when closed

- Status: Accepted (foundation landed: `windowd::destroy_surface` + host-tested `app_surface` model, TASK-0065 P4a). Compositor wiring + chat/search extraction is P4b.
- Created: 2026-06-22
- Builds on: ADR-0028 (windowd surface/present), RFC-0065 (UI v6b app lifecycle), ADR-0036 (service split).
- Code: `source/services/windowd/src/app_surface.rs`, `windowd::WindowServer::{create_surface,destroy_surface}`.

## Context

Today chat and search render into windowd's **shared surface atlas** — one plane holding the
shell chrome plus every app's pixels, all allocated up front (the atlas is sized for chat + blur +
sidebar + search whether or not those windows are open). This is the opposite of how a real OS
manages app surfaces, and it has concrete costs: the atlas is permanently sized for the worst case,
closed apps still consume budget, and an app's pixels are entangled with the shell's.

OpenHarmony (WindowManagerService + each ability's own `Surface`/`BufferQueue`) and Apple (the window
server compositing each app's IOSurface) both give **each app its own buffer**, composited as its own
layer, allocated when the scene becomes active and released when it goes away.

windowd already supports this: `create_surface` gives each caller a `Surface` with its **own**
`SurfaceBuffer` (its own VMO handle + pixels), owner-gated, and the compositor draws `Layer { surface,
x, y, z }` — N independent surfaces with z-order. The baked chat/search are the exception, not a
limitation.

## Decision

Each app owns its **own surface VMO**, **lazily allocated when the app becomes active** (launched /
foregrounded via the `abilitymgr` lifecycle) and **freed when the app closes/stops** — never baked
into a shared plane with the shell or other apps.

- **Allocation is lazy**: an inactive/closed app holds **no** surface VMO. The surface is created at
  activation and `destroy_surface`'d at stop, reclaiming its memory.
- **One layer per app**: each app surface composites as its own `Layer` with its own z-order; the
  shell chrome stays a separate layer below the app layers.
- **Lifecycle drives residency**: the `abilitymgr` launch handoff (`SurfaceBinder`) mounts an app's
  surface on activate and unmounts (frees) it on stop. The pure, host-tested `app_surface::AppSurfaces`
  registry tracks instance → surface id + z + residency, bounded by `MAX_APP_SURFACES`.
- **windowd owns the VMO**, `abilitymgr` owns *when* it exists (authority split per ADR-0036): the app
  process presents into its surface; windowd hosts + composites it; abilitymgr's lifecycle decides
  residency.

## Consequences

- **Positive**: closed apps cost nothing; the shared atlas no longer has to be sized for every possible
  window; an app's pixels are isolated in its own VMO (cleaner, and a prerequisite for chat/search
  becoming real app processes). Matches the OHOS/Apple model.
- **Positive**: `destroy_surface` (free-on-close) + the lazy `app_surface` registry are host-tested in
  isolation before the risky compositor rewrite.
- **Cost / sequencing**: the compositor runtime must stop baking chat/search into the atlas and instead
  composite per-app client surfaces (P4b). Until then the `app_surface` model is host-proven but not yet
  driving the live composite (gated `#[cfg(test)]`).
- **Bounded**: `MAX_APP_SURFACES` caps resident app surfaces (≤ windowd `MAX_SURFACES`).
