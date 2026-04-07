<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Console

Console is the first-party command-line surface for local development, automation, diagnostics, and service-oriented
workflows on Open Nexus OS.

It should feel powerful enough for real work, without becoming a legacy unrestricted shell that quietly owns the whole
machine.

## Role in the platform

Console is not meant to be:

- a hidden "escape hatch" around the platform,
- a generic root shell,
- or a requirement for ordinary users.

Instead, it is meant to be:

- a developer-facing product surface,
- a place to work in project and service contexts,
- a practical home for scripts and automation,
- and a companion to Dev Studio rather than a competing system.

## Expected workflows

Console should be a good fit for:

- project setup,
- build/test commands,
- runtime and tool use,
- package-manager workflows,
- service inspection and control,
- local dev server operation,
- and bounded automation scripts.

Typical examples:

- open a project shell and run local build/test commands,
- inspect logs for a local service,
- start or stop a development database,
- run setup automation for a project,
- or use a package manager from a Console session rather than through only graphical UI.

## Mental model

The preferred mental model is:

- commands run in a **context**, not vaguely "on the machine",
- Console sessions should usually be tied to a project, toolset, or local service,
- and the platform should help developers stay oriented instead of exposing one giant mutable global environment.

This keeps development workflows understandable without requiring the platform to behave like a legacy admin workstation.

## Relationship to the rest of the system

Console should work naturally alongside:

- **Dev Mode**, which makes developer surfaces visible and usable,
- **Package Manager** flows for installing runtimes, tools, and services,
- **Dev Studio**, which may open or reuse Console sessions for build/test/install workflows,
- and **Local Services**, which should be inspectable and manageable from the command line.

## Best practice

- prefer project-scoped sessions over broad machine-wide mutation,
- use the platform's managed install and environment flows where possible,
- treat Console as part of the product, not as a rough compatibility corner,
- and keep automation honest, bounded, and easy to reason about.

## Avoid

- assuming Console implies unrestricted system authority,
- turning every workflow into handwritten shell glue when a clearer platform flow exists,
- or designing Console as a second-class debug toy instead of a real development surface.

## Related docs

- `docs/dev/technologies/dev-mode.md`
- `docs/dev/foundations/development/package-manager.md`
- `docs/dev/foundations/development/dev-studio.md`
- `docs/security/shell-scripts-and-automation.md`
