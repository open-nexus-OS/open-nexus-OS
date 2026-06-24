---
title: "TASK-0080D DSL App Runtime: app-host + RFC-0065 lifecycle/registry/caps bridge + per-app surface (ADR-0037)"
status: Draft
owner: "@ui @runtime"
created: 2026-06-23
depends-on:
  - tasks/TASK-0076B-dsl-v0_1c-visible-os-mount-first-frame.md   # visible DSL mount in windowd/SystemUI
  - tasks/TASK-0079-dsl-v0_3a-aot-codegen-incremental-assets.md  # AOT'd app crates under userspace/apps/generated
  - tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md  # abilitymgr/bundlemgrd/notifd spine
follow-up-tasks:
  - tasks/TASK-0080C-systemui-dsl-bootstrap-shell-os-wiring.md   # launcher launch path consumes this runtime
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSL v1 DevX track: tasks/TRACK-DSL-V1-DEVX.md
  - App lifecycle contract (RFC): docs/rfcs/RFC-0065-ui-v6b-app-lifecycle-registry-notifications-navigation-contract.md
  - Service split (ADR): docs/adr/0036-ability-lifecycle-vs-process-vs-registry-service-split.md
  - Per-app surface lifecycle (ADR): docs/adr/0037-per-app-surface-lazy-vmo-lifecycle.md
  - Registry SSOT (build-time, from manifests): source/services/bundlemgrd/build.rs
  - Launch caps authority: source/services/abilitymgr/src/caps.rs
  - Testing contract: scripts/qemu-test.sh
---

## Context

The DSL track (TASK-0075…0080C) makes apps *authorable* (syntax/IR → interpreter → AOT Rust crates under
`userspace/apps/generated/<app>_dsl/`) and *mountable* into the live shell. But it never defines the **app
runtime itself** — the thing that turns a DSL program into a *real app in the running system*. Today the
launcher tasks (TASK-0080B/C) say "app launch uses the real app lifecycle/service contract" but no task
specifies:

- **What executes the app** (the runtime host) and where it lives (in the shell process vs a separate app
  process),
- **Who owns the app's surface** (the shell's shared atlas vs an app-owned VMO — ADR-0037),
- **How the app is discovered + launched + permission-gated** (the RFC-0065 spine: bundlemgrd registry →
  abilitymgr launch authority → caps), versus the DSL launcher just calling `mount(...)` directly and
  bypassing it.

### The hard constraint this task must record (discovered 2026-06-23)

The kernel's `execd` spawn path **only runs hand-assembled RISC-V syscall stubs** (see
`userspace/apps/demo-exit0/build.rs`, which emits machine code via `encode_addi`/`encode_auipc`; the image
table in `execd/src/os_lite.rs` is `IMG_HELLO`/`IMG_EXIT0`/`IMG_EXIT42`). **It does not run compiled Rust
binaries.** Therefore "a DSL app as a separately spawned ELF process" is NOT available until a real userspace
app runtime exists (heap, no_std entry, syscall surface, linker layout). The v1 app runtime is consequently
**in-process mount** (the DSL App Host runs inside the shell runtime), with a clean boundary so the host can
later move to its own process without changing the app contract.

### What already exists (do not rebuild)

- **Registry from real manifests:** `bundlemgrd` generates `APP_REGISTRY` at build time from
  `bundles/<app>/manifest.toml` (no hand-maintained list; a phantom cannot appear without a manifest).
- **Launch caps authority:** `abilitymgr` resolves an app's manifest-declared `caps` and **fails closed** on
  an unknown permission (`Broker::launch_with_caps` → `STATUS_DENIED`); boot self-check emits
  `abilitymgr: caps ok app=<id> (n=…)`. `KNOWN_PERMISSIONS` = `nexus.permission.{WINDOW,NOTIFY,STATE}`.
- **App owns its data/surface content (ADR-0037 step 1):** `search-app` (`no_std`) owns the search window's
  data; windowd hosts it. The per-app *VMO* surface boundary is the remaining ADR-0037 work.
- **Dynamic Apps menu:** windowd builds the launcher list from the registry (`windowd: apps ok (n=…)`).

## Goal

Define and deliver the **DSL App Runtime** — the contract that makes every DSL app a real, registered,
permission-gated, surface-owning app — bridging the DSL track to the RFC-0065 lifecycle spine.

Deliver:

1. **App Host (`userspace/dsl/nx_app_host` or equivalent):** a bounded runtime that, given an app id, loads
   the app's page graph (interpreter today; AOT `mount_<page>` when available), drives its `Store/reduce/
   @effect/Page` lifecycle, and produces frames into a surface it owns. Host-tested headless first.
2. **Manifest is the app's identity (SSOT):** every DSL app ships `bundles/<app>/manifest.toml`
   (`name`, `abilities`, `caps`, `bundle_type=app`). The registry (`bundlemgrd`) and the launch caps
   (`abilitymgr`) both derive from it — no second source. A DSL app with no manifest does not appear and
   cannot launch.
3. **Launch goes through the spine, not around it:** the launcher (TASK-0080C) launches an app id by asking
   `abilitymgr` (which enforces manifest caps), NOT by calling `mount(...)` directly. abilitymgr resolves the
   app → tells the App Host to mount it → the surface is hosted by windowd. The hop order is observable.
4. **Per-app surface ownership (ADR-0037):** the App Host renders into an app-owned surface (its own VMO /
   `OwnedSurface`), composited by windowd as a per-app layer — lazily acquired on launch, released on close.
   `nexus.permission.WINDOW` gates the surface bind.
5. **Process-boundary forward path (design, not built here):** document the seam so the App Host can become a
   separate process once the userspace app runtime exists; record the asm-stub spawn constraint above as the
   reason v1 is in-process.

## Non-Goals

- A general compiled-Rust process spawner / userspace ELF runtime (separate, larger track; this task only
  records the seam and constraint).
- Migrating chat to a DSL app (search is the first; chat follows).
- Quick Settings / full SystemUI migration (owned by the SystemUI DSL phases).
- Notifications/state permission *enforcement* beyond declaring `NOTIFY`/`STATE` in `KNOWN_PERMISSIONS`
  (their service-side gating is RFC-0065 follow-up).

## Constraints / invariants (hard requirements)

- **Manifest is the only source of an app's identity + caps.** Registry and launch authority both derive from
  it; never hardcode an app list or its permissions.
- **Launch is capability-gated and fail-closed:** an app requesting an unknown/denied permission is refused
  with a clear marker, never silently mounted.
- **One launch path:** the launcher routes through `abilitymgr`; no direct `mount()` shortcut in the live
  launcher (a host fixture may mount directly for snapshot tests only).
- **App owns its surface; the compositor hosts it** (ADR-0037) — apps do not draw into the shell's shared
  atlas.
- **Bounded:** per-frame interpreter/host work is capped (node + event-queue caps, per TASK-0076); no
  unbounded scripting.
- **In-process is the v1 runtime** (asm-stub spawn constraint); the app contract must not assume same-process
  so the later out-of-process move is a wiring change, not an app rewrite.

## Stop conditions (Definition of Done)

### Host — required

- App Host mounts a DSL app by id and produces a deterministic first frame (snapshot parity with the shared
  visible proof surface). Marker: `dsl: app-host mount ok`.
- Launch authority: launching an app whose manifest declares only known caps succeeds; an app declaring an
  unknown permission is denied (reuses `abilitymgr::caps`). Host test asserts both.
- Registry/manifest round-trip: the app id served by the registry matches a real `bundles/<app>/manifest.toml`
  (cross-checked, like the existing nxb-pack `repo_bundles` test).

### Proof (OS/QEMU) — required

UART markers (in hop order):

- `windowd: apps ok (n=…)` — launcher list from the registry (exists).
- `abilitymgr: caps ok app=<id> (n=…)` — launch authority validated the app's manifest caps (exists).
- `abilitymgr: launch (app=<id>, inst=…)` — launch authorized + lifecycle instance created.
- `dsl: app-host mount on` — the App Host mounted the app's page graph.
- `dsl: app surface visible` — the app-owned surface is composited by windowd and on screen.

Live QEMU pointer click on the launcher entry must drive the chain (not a selftest-only mutation).

## Phasing

- **R1 — App Host (host-tested):** the bounded runtime + `mount(app_id)` over the interpreter; snapshot proof.
- **R2 — Lifecycle bridge:** launcher → `abilitymgr` launch (caps enforced) → App Host mount; the
  `mount()`-shortcut removed from the live path. Reuses the done registry + caps work.
- **R3 — Per-app surface (ADR-0037):** app-owned VMO surface composited by windowd as a layer; lazy
  acquire/release; `WINDOW` permission gates the bind. (search is the first real app surface.)
- **R4 — Process boundary (design/seam only):** document + stub the seam for an out-of-process App Host,
  blocked on the userspace app runtime (asm-stub constraint). No spawn built here.

## Notes

This task is the missing center of the "real app runtime": the DSL gives apps a *shape*, RFC-0065 gives them
*identity + lifecycle + permission*, ADR-0037 gives them a *surface*. TASK-0080D is the contract that binds
the three so "a DSL app" and "a real app in the OS" are the same thing. Every app (search first, then chat,
then the hard apps) ships as a DSL app + manifest and runs on this host.
