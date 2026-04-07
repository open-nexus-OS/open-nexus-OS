---
title: TRACK Developer Experience Surfaces (Developer Features + shell posture): consumer-simple by default, full dev mode when enabled
status: Draft
owner: @devx @ui @runtime
created: 2026-04-07
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Dev Studio (IDE) keystone: tasks/TRACK-DEVSTUDIO-IDE.md
  - App Store umbrella: tasks/TRACK-APP-STORE.md
  - Zero-Copy App Platform: tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Group and Device Management: tasks/TRACK-GROUP-AND-DEVICE-MANAGEMENT.md
  - Settings Family Mode: tasks/TRACK-SETTINGS-FAMILY-MODE.md
  - Typed settings registry: tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - SystemUI Settings DSL: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - UI profiles / shell posture: docs/dev/ui/foundations/layout/profiles.md
  - Security standards: docs/standards/SECURITY_STANDARDS.md
---

## Goal (track-level)

Deliver a first-party **Developer Experience Surfaces** layer that lets Open Nexus OS stay:

- **simple and calm for default users**,
- **powerful for developers when explicitly enabled**,
- and **consistent with the same design system and shell model** rather than becoming a second OS hidden inside the first.

This track exists so that Console, Dev Studio, runtime installers, local service tools, diagnostics, and advanced
developer affordances can be:

- discoverable when desired,
- absent when not needed,
- policy-gated and auditable,
- and visually aligned with the platform's consumer-first posture.

## Scope boundaries (anti-drift)

- This is **not** a second desktop product line or a forked "pro shell".
- This is **not** permission theater where features are merely hidden in UI but still effectively ambient.
- This is **not** a generic admin/root mode modeled after legacy Unix desktops.
- This track defines **visibility + posture + product integration**; install/runtime/service authority remains with the
  relevant platform authorities.

## Product stance

Open Nexus OS should be able to feel:

- like a minimal consumer device for everyday users,
- like a serious development machine for developers,
- and like both are the same platform rather than two separate personalities bolted together.

Developer tooling should therefore appear as a **controlled posture** of the same shell:

- same design language,
- same SystemUI/runtime contracts,
- same settings model,
- but different visible surfaces, affordances, and privileges.

## Core principles

### 1) Visibility and enablement are separate

Developer surfaces need two distinct controls:

- **visibility**: whether Console / Dev Studio / runtime management UI appears,
- **enablement**: whether the corresponding capability set is actually active.

This avoids both security theater and UX confusion.

### 2) Developer mode is typed, not ad-hoc

Preferred posture model:

- `off`: no developer surfaces visible by default
- `basic`: Console, logs, project shells, and Dev Studio visible
- `advanced`: runtime/tool installation, local service capsules, automation shells
- `managed`: developer features available under organization/family/policy limits

The exact storage contract should remain typed and deterministic through `settingsd`.
Managed here should be read broadly:

- household/guardian restrictions,
- school or enterprise profiles,
- fleet / kiosk / device-group posture,

all using the same visibility/enablement split rather than separate shell products.

### 3) The shell should expand, not fork

The system should prefer:

- one canonical shell root,
- one design language,
- one settings surface,
- and one profile/posture model,

rather than a hidden "legacy desktop mode" with unrelated rules.

### 4) Developer affordances should remain policy-gated

Enabling developer features must not bypass:

- `policyd` capability checks,
- package verification/trust rules,
- service registration authority,
- or auditability of sensitive operations.

## Surface model

### Developer Features settings

The platform should expose a dedicated **Developer Features** area in Settings for:

- developer mode level,
- Console visibility,
- Dev Studio visibility,
- runtime/tool installer visibility,
- local services visibility,
- sideload/self-built bundle policy,
- diagnostic/log surfaces,
- optional network/build sandboxes.

This Settings posture should remain compatible with:

- Family Mode restrictions and guardian approvals,
- managed school/enterprise/fleet defaults,
- and per-device or per-group policy overrides delivered through the broader management substrate.

### Launcher / navigation behavior

When developer mode is off:

- Console and Dev Studio are absent from primary launcher/navigation,
- runtime/service management surfaces are not promoted,
- the platform remains consumer-first.

When developer mode is enabled:

- Console and Dev Studio can appear as first-party apps,
- Settings gains a developer category,
- search/deeplink/navigation can expose developer flows intentionally.

### Shell posture

Developer affordances should integrate with the existing shell posture direction:

- a device may stay visually "desktop" or "tablet",
- while `device.shellMode` / developer posture enables extra affordances,
- without inventing a separate always-on "developer OS product".

## Security and policy stance

The track must preserve platform invariants:

- no ambient authority,
- deny-by-default access,
- explicit grants for sensitive developer operations,
- auditable activation of local services / shells / automation flows,
- family/org policy compatibility.

Examples:

- making Console visible must not implicitly grant package install rights,
- enabling runtime installers must not allow unsigned ambient daemons,
- managed mode may allow Dev Studio but deny sideloaded services.

## Relationship to adjacent tracks

- **Dev Studio** remains the main developer workflow product.
- **Console and Toolchains** defines the runtime/tool/service substrate underneath.
- **App Store** continues to own store-facing install/distribution flows.
- **Settings** remains the typed user-facing authority for developer posture preferences.
- **Group and Device Management** defines how household, school, enterprise, fleet, kiosk, and similar managed postures
  can constrain or shape Developer Features without introducing a second shell or policy stack.
- **Settings Family Mode** provides the simple household-facing UI for a subset of those managed controls.

## Phase map

### Phase 0 - visibility posture

- Define developer mode levels and UX principles.
- Establish visibility vs enablement separation.
- Add shell/navigation posture direction for developer surfaces.

### Phase 1 - first-party developer surfaces

- Console, Dev Studio, logs, diagnostics, and runtime manager appear coherently when enabled.
- Search/deeplink/launcher behavior is defined for developer tools.

### Phase 2 - policy-managed developer device

- Family / org / education policy can constrain developer features without breaking the shell model.
- Managed developer mode becomes a first-class product stance.

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-DEVSURF-000: Developer Features settings model v0 (typed levels + visibility/enablement split)**
- **CAND-DEVSURF-010: SystemUI / Launcher developer posture wiring v0**
- **CAND-DEVSURF-020: Developer search/deeplink/navigation surfaces v0**
- **CAND-DEVSURF-030: Managed developer mode policy integration v0**
