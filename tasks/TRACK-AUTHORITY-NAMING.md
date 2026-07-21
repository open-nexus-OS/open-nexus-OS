---
title: TRACK Authority & Naming Registry (single-source-of-truth)
status: Draft
owner: @runtime
created: 2025-12-30
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Keystone gates: tasks/TRACK-KEYSTONE-GATES.md
  - Authority decision (binding): tasks/TASK-0266-architecture-v1-authority-naming-contract.md
---

## Purpose

This track is the **single source of truth** for:

- canonical service/daemon names (“authorities”),
- naming conventions (to prevent drift),
- canonical URI schemes and artifact formats,
- CLI naming conventions.

This exists to remove “warnings” by making the architecture **decided** and mechanically followable.

## Naming rules (hard)

- **Daemons/services** use `*d` suffix (e.g. `logd`, `policyd`, `powerd`).
- **Libraries** use descriptive crate names, no `*d` suffix.
- **Legacy/placeholder** names (e.g. `*mgr`) may exist in the repo during bring-up, but they must **not** be treated as authorities.
  In planning, we standardize on the canonical names below; when implementation lands, placeholders must be **replaced/renamed/removed** rather than extended.
- **No parallel authorities**: if a new task needs functionality owned by an authority below, it must **extend** that authority (or introduce a replacement with an explicit deprecation plan).
- **No new URI schemes** without an explicit design decision recorded here.
- **No new on-disk “contracts”** without an explicit format registry entry here.

## Canonical authorities (v1 direction)

### Runtime & system

- **Init/orchestrator**: `nexus-init` (not a daemon; canonical init role)  
- **Spawner**: `execd`
- **Service registry**: `samgrd`
- **Policy authority**: `policyd`
- **Audit/log sink**: `logd`
- **Persistence substrate**: `statefsd` (authority for durable `/state` key/value semantics)

### UI & graphics

- **Window management + composition + present**: `windowd` (single compositor authority)
- **Display framebuffer access**: `fbdevd`
- **Rendering abstraction**: `renderer` (library; backend trait + cpu2d; future GPU backend behind the same contracts)

### Input & text

- **Input event routing**: `inputd`
- **HID device driver**: `hidrawd`
- **Touch device driver**: `touchd`
- **IME engine**: `imed` (**canonical**; `ime` is a deprecated placeholder name — deletion scheduled in TASK-0147 Part 1, RFC-0075)
- **IME UI (OSK + candidate strip)**: `ime-ui` DSL overlay app (`userspace/apps/ime-ui`) — never inside `windowd` (compositor stays UI-free)
- **Locale/i18n**: **no daemon by design** (RFC-0077) — settingsd owns `ui.locale`, windowd relays via `OP_SURFACE_REGION`, app-host applies locale packs; an `l10nd` service is explicitly rejected

### Time

- **Monotonic timers + wall-clock (UTC)**: `timed` (single time authority; walltime = RTC anchor + monotonic delta, RFC-0076)
- **RTC hardware access**: `rtc-goldfish` LIBRARY in `source/drivers/rtc/goldfish-rtc` — consumed by `timed` directly (RFC-0076 deviation: no rtcd service; the time authority reads its own anchor)
- **Network time sync**: `time-syncd` (placeholder today; SNTP seed = TASK-0299; may only refine timed's anchor, never a second clock authority)
- **Timezone conversion**: `tz-lite` (client-side library, no service; zone table = validator SSOT for `time.zone`)

### Power & device health

- **Power governor + wakelocks + standby**: `powerd` (**canonical**; `powermgr` is deprecated placeholder)
- **Battery/fuel gauge**: `batteryd` (**canonical**; `batterymgr` is deprecated placeholder)
- **Thermal management**: `thermald` (**canonical**; `thermalmgr` is a placeholder)

### App lifecycle

- **App lifecycle broker / ability lifecycle**: `appmgrd` (**canonical**; `abilitymgr` is a placeholder)

### Media

- **Audio mix/route**: `audiod`

### Storage & content

- **Content broker**: `contentd`
- **Scoped grants**: `grantsd`
- **Trash + file ops**: `trashd`, `fileopsd`

### Sensors & buses

- **I²C bus**: `i2cd`
- **SPI bus**: `spid` (sim-first)
- **Sensor aggregation**: `sensord`
- **Location authority (apps consume)**: `locationd`
- **GNSS device driver**: `gnssd`

### Recovery / provisioning

- **Recovery orchestration**: `recovery-init` + `recovery-sh` (built-ins only)
- **Flashing (recovery target)**: `flashd`
- **Next-boot selector**: `rebootd`

## Canonical URI schemes (v1 direction)

- **Packages**: `pkg://...` (read-only)
- **Content broker**: `content://<provider>/<docId>` (no raw filesystem paths exposed to apps)
- **State**: `state:/...` (authority is `statefsd`; paths are *naming*, not direct POSIX exposure)
- **Data URLs (inputs only)**: `data:` (allowed only where explicitly policy-gated)

## Canonical artifact formats (v1 direction)

- **App bundle**: `.nxb` with `manifest.nxb` as the canonical manifest (JSON only as derived view)
- **Policy snapshot**: `policy.bin` as a derived artifact (authority remains `policyd`)
- **Recovery action token**: `.nxra` (signed, replay-protected)
- **Crash artifacts**: `.nxcd.zst` (existing direction; no parallel dump formats without decision)

## CLI naming (hard)

- **Single CLI entrypoint**: `nx` (host tool). New functionality must be added as `nx <topic> ...`.
- Optional shims like `nx-image` / `nx-flash` are allowed **only** as thin wrappers that forward to `nx image` / `nx flash` (no duplicate logic).

## Replacement rules (planning-first; no “deprecation ceremony”)

When a placeholder or legacy daemon name exists in the repo:

- Do not add new dependencies on it.
- Tasks must name and wire the **canonical authority** (this registry).
- Implementation must **replace/rename/remove** the placeholder so only the canonical authority remains.
  (Because we are still in planning/bring-up, we do not promise compatibility with placeholder names.)

## Known repo placeholders (inventory; must be replaced)

This section exists only to remove ambiguity between **repo reality** and **planned end-state**.
It is not a promise of compatibility for placeholder names.

- **`source/services/powermgr/`** → `powerd` (see `TASK-0236`/`TASK-0237`)
- **`source/services/batterymgr/`** → `batteryd` (see `TASK-0256`/`TASK-0257`)
- **`source/services/thermalmgr/`** → `thermald` (see `TASK-0272`/`TASK-0271`)
- ~~**`source/services/ime/`** → `imed`~~ — DONE 2026-07-21: placeholder deleted, `imed` is real (TASK-0147, RFC-0075)
- **`source/services/abilitymgr/`** → `appmgrd` (see `TASK-0065`/`TASK-0235`)
- **`source/services/compositor/`** → removed in favor of `windowd` (see `TASK-0055`/`TASK-0170`/`TASK-0251`)

Implementation note: the concrete placeholder replacement pass is tracked in `TASK-0273`.
