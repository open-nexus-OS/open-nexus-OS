<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Development Foundations

This section explains the platform-wide foundations for local development on Open Nexus OS.

It is for questions like:

- how Console is meant to feel and behave,
- how the package manager and install model should work for developers,
- how Dev Studio fits into the same system,
- and how runtimes, tools, local services, and automation should fit together without falling back to legacy Unix chaos.

Current entry points:

- `docs/dev/foundations/development/console.md`
- `docs/dev/foundations/development/package-manager.md`
- `docs/dev/foundations/development/dev-studio.md`

This section is more technical than `docs/dev/technologies/`, but it should still stay readable for app developers and
students rather than turning into kernel or daemon internals.

Rule of thumb:

- `technologies/` explains the product promise,
- `foundations/development/` explains the cross-cutting developer model,
- and `security/`, `packaging/`, `architecture/`, and `adr/` explain the deeper contracts underneath.
