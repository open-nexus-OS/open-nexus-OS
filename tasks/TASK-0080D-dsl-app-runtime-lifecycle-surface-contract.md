---
title: "TASK-0080D DSL App Runtime: app-host process + packaging (.nxir in .nxb) + execd spawn + cross-process surface (ADR-0042)"
status: Draft
owner: "@ui @runtime"
created: 2026-06-23
updated: 2026-07-06
depends-on:
  - tasks/TASK-0076B-dsl-v0_1c-visible-os-mount-first-frame.md   # visible mount + the execd isolation probe
  - tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md          # svc adapters the effect host speaks
  - tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md  # abilitymgr/bundlemgrd spine
follow-up-tasks:
  - tasks/TASK-0080C-systemui-dsl-bootstrap-shell-os-wiring.md   # launcher e2e consumes this runtime
  - tasks/TASK-0079-dsl-v0_3a-aot-codegen-incremental-assets.md  # AOT ELFs ride the same payloadKind dispatch
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Surface transport (drafted, finalize here): docs/adr/0042-cross-process-surface-transport.md
  - Per-app surface ownership: docs/adr/0037-per-app-surface-lazy-vmo-lifecycle.md
  - Service split: docs/adr/0036; lifecycle RFC: docs/rfcs/RFC-0065
  - Display wire SSOT: source/libs/nexus-display-proto (ADR-0038)
  - Packaging (EXISTS): docs/packaging/nxb.md, tools/nexus-idl/schemas/manifest.capnp,
    tools/nxb-pack, source/services/{bundlemgrd,packagefsd}
  - Spawn path (EXISTS, probe-gated): source/services/execd + userspace/nexus-loader
  - Lifecycle contract page: docs/dev/dsl/runtime.md
---

## Context (updated 2026-07-06)

**Masterplan decision: the v1 app runtime is a real separate process** — one optimized
`app_host` runtime ELF; execd spawns it per app; it loads the app's compiled `.nxir`
(zero-parse capnp) from the installed bundle and presents through its own
cross-process windowd surface (ADR-0042). This supersedes this task's earlier
"v1 = in-process host" fallback.

**Reality check on the spawn constraint (corrected):** the 2026-06-23 note said execd
only runs hand-assembled stubs. That describes the os-lite demo image table; the real
ELF spawn path EXISTS (`exec_elf`: GET_PAYLOAD → nexus-loader `parse_elf64_riscv` →
`as_create` Sv39 → VMO staging → W^X → `spawn` + `cap_transfer` + restart reaper) but
is **not the live boot flow** and carries a contradictory shared-address-space comment.
That is exactly what the **TASK-0076B isolation probe** settles first. If the probe
fails, a dedicated execd-hardening sub-task blocks this task (and only this task —
everything else is host-side).

What already exists and is built on, not around: build-time `APP_REGISTRY` from
`bundles/<app>/manifest.toml`; abilitymgr `launch_with_caps` fail-closed
(`KNOWN_PERMISSIONS = nexus.permission.{WINDOW,NOTIFY,STATE}` + QUERY from 0078B);
`.nxb` bundles + `nxb-pack` (payload today = placeholder ELF); packagefsd;
windowd `create_surface`/`destroy_surface` + `app_surface` residency (ADR-0037).

## Goal

1. **Packaging**: `manifest.capnp` v2.1 gains `payloadKind` (append-only field;
   `elf | uiProgram`); `nxb-pack` packs `payload.nxir` for DSL apps; bundlemgrd
   install/digest/enumerate unchanged (payload-agnostic).
2. **app_host** (`source/services/app-host/`, no_std, modules pinned: `payload.rs`,
   `surface.rs`, `input.rs`, `render_loop.rs`, `effect_host/` (one module per svc),
   `persist.rs`): argv `<name@ver>` → fetch payload (v1 GET_PAYLOAD; packagefs
   VMO-map recorded as the zero-copy upgrade) → validate IR (nexus-dsl-ir, fail-closed)
   → mount (nexus-dsl-runtime) → ADR-0042 surface → render loop (nexus-layout +
   renderer into the surface VMO; arenas at mount, steady-state zero-alloc,
   debug alloc-assert) → present with damage → input events in → effects over real
   IPC adapters.
3. **execd dispatch**: `payloadKind == uiProgram` ⇒ spawn app_host with the app id +
   transfer the granted caps (windowd client cap gated by `WINDOW`, statefsd by
   `STATE`, queryd by `QUERY`); restart policy per manifest.
4. **windowd transport (ADR-0042 finalized here)**: `SURFACE_CREATE/PRESENT/DESTROY`
   in nexus-display-proto; damage-blit into the atlas region backing the app's layer;
   seq/ack flow control; input routed by surface id. New windowd code in its own
   modules (`client_surface.rs`, `surface_transport.rs`) — no godfile growth.
5. **Lifecycle**: RFC-0065 hop order observable (registry → caps → launch → mount →
   visible); suspend persists `@persist` fields via statefsd; stop destroys the
   surface (ADR-0037 lazy residency); reaper handles crashes per restart policy.
6. **Probe-first sequencing**: R1 is a solid-color app_host (no DSL) proving
   spawn + VMO + present before anything else.

## Non-Goals

- Launcher/shell wiring (0080C). AOT codegen (0079 — but its ELFs launch through the
  same payloadKind dispatch, forward-compatible here). Notifications/state permission
  service-side enforcement beyond declaration (RFC-0065 follow-up). Migrating
  chat/search content apps (follow-up wave). Kernel changes (probe-driven execd fixes
  excepted, staged separately if needed).

## Constraints / invariants (hard requirements)

- **Manifest = the app's only identity/caps source**; registry + launch authority
  derive from it; a DSL app without a manifest does not exist.
- **Fail-closed everywhere**: unknown permission ⇒ denied (existing); invalid/tampered
  IR (hash/type/budget) ⇒ deterministic launch error, never a partial mount.
- **One launch path** through abilitymgr — no `mount()` shortcut in live paths (host
  fixtures may mount directly for snapshots only).
- **Apps get pixels + events, nothing else**: no scene-graph access, no shell atlas
  writes (information hiding at the process boundary; ADR-0037/0042).
- Bounded: per-frame work capped by IR budgets; one in-flight present; damage list
  capped; `MAX_APP_SURFACES` respected.
- Steady-state zero heap allocation in app_host (bump-allocator rule; debug assert).
- No `unwrap/expect`; no godfiles; no company/product names.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- app_host logic host-tested: payload validate/mount against fixtures; effect host
  against transcripts; surface module against a stub sink (frame bytes golden);
  manifest payloadKind round-trip (nxb-pack → parse); registry/manifest cross-check
  (extends the existing `repo_bundles` test).

### Proof (OS/QEMU) — required (user boot-verify, staged)

Marker chain in hop order:

- R1: `APPHOST: probe surface presented` + visible solid-color window (no DSL)
- `windowd: apps ok (n=…)` → `abilitymgr: caps ok app=<id> (n=…)` →
  `abilitymgr: launch (app=<id>, inst=…)` → `APPHOST: mounted <name>@<ver> hash=<h>` →
  `WINDOWD: surface presented id=<n> seq=<k>` → `dsl: app surface visible`
- input: click inside the app window dispatches an event visibly (state change on
  screen); no background leak to the shell
- suspend/stop: `APPHOST: persisted (fields=<n>)` + surface destroyed
  (ADR-0037 marker); crash → reaper restart per policy
- denied fixture app (unknown permission) refused with the existing denial marker

### Docs — required

- ADR-0042 Status → Accepted (with any deviations recorded);
  `docs/dev/dsl/runtime.md` lifecycle/launch sections final;
  `docs/packaging/nxb.md` payloadKind section.

## Phasing (each boot-verified)

- **R1 — transport probe**: solid-color app_host spawned by execd, ADR-0042
  create/present, visible window. De-risks everything.
- **R2 — real payload**: manifest v2.1 + nxb-pack `.nxir` + bundlemgrd fetch +
  validate/mount + first DSL app frame (counter demo).
- **R3 — interaction + lifecycle**: input routing, effects over real IPC, `@persist`
  suspend/restore, stop/crash residency.
- **R4 — AOT forward-compat**: payloadKind dispatch verified against a generated-ELF
  bundle (lands with 0079/0080).

## Notes

The DSL gives apps a shape; RFC-0065 gives identity + lifecycle + permission;
ADR-0037/0042 give a surface. This task binds them so "a DSL app" and "a real app in
the OS" are the same thing — search first, then chat, then the hard apps.
