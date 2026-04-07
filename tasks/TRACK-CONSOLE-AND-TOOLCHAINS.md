---
title: TRACK Console and Toolchains (secure shell + runtimes + tools + local services): real developer power without Unix ambient authority
status: Draft
owner: @devx @runtime @security
created: 2026-04-07
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Dev Studio (IDE) keystone: tasks/TRACK-DEVSTUDIO-IDE.md
  - App Store umbrella: tasks/TRACK-APP-STORE.md
  - Zero-Copy App Platform: tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Group and Device Management: tasks/TRACK-GROUP-AND-DEVICE-MANAGEMENT.md
  - Settings Family Mode: tasks/TRACK-SETTINGS-FAMILY-MODE.md
  - Packages install authority: tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Installer v1.1 direction: tasks/TASK-0239-installer-v1_1b-os-pkgr-atomic-ab-bundlemgr-registry-licensed-selftests.md
  - Typed settings registry: tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - Service architecture: docs/adr/0017-service-architecture.md
  - bundlemgrd onboarding: docs/architecture/15-bundlemgrd.md
  - samgrd onboarding: docs/architecture/14-samgrd-service-manager.md
  - Security standards: docs/standards/SECURITY_STANDARDS.md
  - Build standards: docs/standards/BUILD_STANDARDS.md
---

## Goal (track-level)

Deliver a first-party **Console and Toolchains** substrate that lets Open Nexus OS behave like a real development machine
for local software work, while preserving the platform's core security model:

- **real console workflows** for developers, automation, and server tools,
- **installable runtimes and tools** such as Python/Node/package managers,
- **local service capsules** for things like databases, dev servers, language servers, and build helpers,
- and **sandboxed script/build execution** for legacy install flows,

without inheriting the ambient-authority, postinstall-chaos model of traditional Unix desktops.

## Scope boundaries (anti-drift)

- This is **not** "ship a Linux compatibility layer and call it done".
- This is **not** ambient root, mutable global PATH chaos, or unrestricted daemon sprawl.
- This is **not** a second install authority parallel to `bundlemgrd`.
- This is **not** a promise that arbitrary legacy scripts get full machine access.

## Product stance

Open Nexus OS should be able to say:

- yes, you can develop locally,
- yes, you can install runtimes, package managers, and server tools,
- yes, shell automation is supported,
- but all of that happens through **explicit contracts, bounded sandboxes, and capability-gated sessions**.

The intended result is:

- more open than iOS-style lock-down,
- less chaotic than legacy Unix package/shell models,
- and more auditable than ad-hoc developer workstations.

## Core principles

### 1) Console is a product surface, not an escape hatch

The Console should be treated as a first-party platform surface with:

- shell sessions,
- logs,
- pipes/job control,
- structured tool execution,
- and project/service-aware contexts,

rather than as an unrestricted backdoor into the whole machine.

### 2) Commands run in a context, not "on the machine"

Preferred execution contexts:

- `user shell`
- `project shell`
- `automation shell`
- `service shell`
- `recovery shell`

Each session should carry an explicit capability profile and scope.

### 3) Package kinds must expand beyond apps

The platform should support installable artifact classes such as:

- `app`
- `runtime`
- `tool`
- `service`
- `provider`
- optional `sdk`

Examples:

- `python` / `node` are `runtime`
- `npm` / `pip` / `git` / `clang` are `tool`
- `postgres` / `redis` / language servers are `service`

### 4) Legacy script compatibility belongs in a sandbox

Developers need shell scripts, installers, and build automation. The platform should support them, but only through:

- build/install sandboxes,
- bounded filesystem scopes,
- bounded network profiles,
- no implicit system-wide mutation,
- and explicit artifact capture into managed environments or service capsules.

### 5) Authority stays single and explicit

- **Install authority** stays with `bundlemgrd` / installer flow.
- **Service registration** stays with `samgrd`.
- **Policy** stays with `policyd`.
- **Settings / visibility posture** stays with `settingsd` + SystemUI.

No new "developer shortcut" should silently duplicate these authorities.
The same rule should hold for managed environments:

- family restrictions,
- school/enterprise controls,
- fleet / kiosk profiles,

must shape developer tooling through the existing authorities rather than a parallel management bypass.

## Console model

### Session classes

The Console product should support at least:

- **project shell**: scoped to one project/workspace and its managed environments
- **tool shell**: runs a specific toolset/runtime set with bounded access
- **service shell**: attach/manage a specific local service capsule
- **automation shell**: non-interactive recipe/script execution with explicit sandbox profile
- **recovery shell**: tightly constrained diagnosis/repair shell

### Capability posture

Shell sessions should receive explicit grants such as:

- project/home/temp filesystem access,
- runtime usage,
- process spawn rights for an allowed toolset,
- package resolution/install rights,
- service management rights for a namespace,
- bounded network access,
- bounded secret/token handles.

No shell session should implicitly receive global system authority.

## Runtimes, tools, and services

### Managed runtimes

The platform should support installable/versioned runtimes such as:

- Python
- Node.js
- Java/.NET/Deno later if desired

These should be:

- installable via the package authority,
- versioned and updatable,
- usable from project or user environments,
- and removable/rollback-safe.

### Managed tools

Developer tools such as package managers, compilers, linters, and CLIs should be:

- installable as `tool` artifacts,
- scoped to user/project/service contexts,
- and prevented from mutating the whole system by default.

### Local service capsules

Server-like tools and developer daemons should be modeled explicitly:

- databases
- cache servers
- dev web servers
- language servers
- build/watch helpers

These should have:

- lifecycle control,
- data directories,
- port/network policy,
- logs/attach/stop/restart semantics,
- and dependency visibility.

## Automation and installer stance

### Native brokered flow

The preferred path is:

- declarative or structured install/build actions,
- project environment operations,
- service enable/disable actions,
- and policy-aware runtime management.

### Compatibility flow

When legacy shell scripts or package-manager hooks are needed:

- run them inside a build/install sandbox,
- capture outputs as managed artifacts or environment revisions,
- require explicit opt-in for network/native build access,
- and never grant unrestricted machine mutation as a side effect.

This is especially important for ecosystems like:

- `npm` with lifecycle scripts,
- `pip` with native builds,
- build systems invoking compilers/linkers,
- and self-hosted server tooling.

## Safety and policy stance

The track must preserve:

- no ambient authority,
- deny-by-default package/service rights,
- auditable service activation and package installs,
- deterministic rejection for unsupported or over-broad operations,
- compatibility with family/org/device policy.

Examples:

- a visible Console must not imply tool install rights,
- a package manager tool must not register background services directly,
- a local service must not bind privileged ports or broad filesystem roots without explicit policy.
- family, school, enterprise, and fleet policy should be able to narrow these workflows without requiring a different
  console/toolchain architecture for each managed posture.

## Relationship to adjacent tracks

- **Developer Experience Surfaces** owns visibility, settings posture, and user-facing surfacing.
- **Dev Studio** uses this substrate rather than inventing its own runtime/install model.
- **App Store** remains the distribution umbrella for store-visible artifacts and trust channels.
- **Zero-Copy App Platform** can consume these capabilities for advanced apps and provider ecosystems.
- **Group and Device Management** should be able to constrain or preconfigure runtimes, tools, services, and automation
  posture through the same package/policy/config model.
- **Settings Family Mode** is the household-facing subset that may, for example, disable Dev Mode, sideloading, or
  selected runtime/tool installation paths for child profiles.

## Phase map

### Phase 0 - contract framing

- Define artifact classes (`runtime`, `tool`, `service`, etc.).
- Define console session classes and capability posture.
- Define the difference between brokered actions and compatibility sandboxes.

### Phase 1 - usable local developer machine

- First-party Console exists.
- Managed runtimes/tools are installable and usable in project shells.
- Basic local service capsules and automation shells exist.

### Phase 2 - ecosystem hardening

- Safer compatibility for package-manager/build ecosystems.
- Rollback, audit, and policy controls mature.
- Enterprise/managed developer workstation posture becomes credible.

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-CONSOLE-000: Console surface v0 (session classes + capability-aware exec model)**
- **CAND-CONSOLE-010: Runtime/tool artifact contract v0**
- **CAND-CONSOLE-020: Project environments v0 (managed runtime/tool revisions)**
- **CAND-CONSOLE-030: Local service capsules v0 (ports, data dirs, lifecycle, logs)**
- **CAND-CONSOLE-040: Automation/install sandbox v0 (legacy shell scripts + package-manager hooks)**
