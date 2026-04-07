<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Development

This section explains the development-facing foundations that affect visible developer surfaces in Open Nexus OS.

It is for questions like:

- how Console is meant to feel and behave as a product surface,
- how the package manager and install model should work for developers,
- how Dev Studio fits into the same visible system,
- and how developer workflows should stay coherent with the platform's broader UX and safety model.

Current entry points:

- `docs/dev/ui/foundations/development/console.md`
- `docs/dev/ui/foundations/development/package-manager.md`
- `docs/dev/ui/foundations/development/dev-studio.md`

This section is more technical than `docs/dev/technologies/`, but it should still stay readable for app developers and
students rather than turning into daemon internals or low-level architecture contracts.

Rule of thumb:

- `technologies/` explains the product promise,
- `ui/foundations/development/` explains the cross-cutting developer model for visible surfaces,
- and `security/`, `packaging/`, `architecture/`, and `adr/` explain the deeper contracts underneath.
