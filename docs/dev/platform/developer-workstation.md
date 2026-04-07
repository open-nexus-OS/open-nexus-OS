<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Developer Workstation

This page maps the main pieces that make Open Nexus OS behave like a credible local development machine without
abandoning the platform's capability-first security model.

Scope: this is a **platform map**, not a low-level implementation contract.

## Core stance

The intended developer machine story is:

- consumer-simple by default,
- developer-powerful when intentionally enabled,
- and safer, more explicit, and less chaotic than traditional ambient-authority workstation models.

## Main pillars

### Pillar 1: Dev Mode

Dev Mode is the product-facing posture that makes developer surfaces visible and usable:

- Console
- Dev Studio
- runtime and tool installation
- local service workflows
- sideload and self-built developer flows

### Pillar 2: Console

Console is the command-line surface for:

- project work,
- automation,
- package-manager flows,
- diagnostics,
- and local service interaction.

### Pillar 3: Package Manager

The package manager should handle installable developer artifacts such as:

- apps,
- runtimes,
- tools,
- and local services.

### Pillar 4: Dev Studio

Dev Studio should orchestrate the same developer substrate rather than inventing parallel workflows for installs, tools,
or execution.

## Installable kinds

The long-term workstation story depends on installable classes that are more expressive than end-user apps alone:

- **app**
- **runtime**
- **tool**
- **service**

These distinctions help the platform express things like:

- Python or Node as runtimes,
- npm or git as tools,
- and databases or language servers as local services.

## Development posture

The developer workstation posture should favor:

- project-aware execution,
- visible runtime/tool versions,
- explicit local services,
- and safe automation,

instead of one giant machine-wide mutable state pile.

## Security posture

Open Nexus OS should still preserve:

- no ambient authority,
- policy-gated sensitive operations,
- explicit install and service authority,
- and a path for scripting and automation that stays bounded and auditable.

## Related docs

- `docs/dev/technologies/dev-mode.md`
- `docs/dev/foundations/development/README.md`
- `docs/security/capabilities.md`
- `docs/security/shell-scripts-and-automation.md`
- `docs/packaging/artifact-kinds.md`
