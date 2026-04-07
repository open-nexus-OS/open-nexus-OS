<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Developer Docs

This section is the **developer-facing** documentation for Open Nexus OS.

The structure is intentionally split by **kind of decision**:

- `docs/dev/dsl/` explains the language, IR, runtime, QuerySpec, and service-facing DSL contracts.
- `docs/dev/foundations/` explains cross-cutting developer foundations such as local development posture, Console,
  package-management, and Dev Studio expectations.
- `docs/dev/ui/` explains visible UI behavior, system surfaces, components, collections, and cross-device UX rules.
- `docs/dev/technologies/` explains platform technologies from the usage/integration side rather than from daemon internals.
- `docs/dev/platform/` covers platform maps and keystone-style cross-subsystem reference material.

Goals:

- fast onboarding,
- stable contracts (determinism, boundedness, purity rules),
- clear separation between UI guidance, DSL guidance, and technology guidance,
- and “proof-first” references to tests, goldens, and markers where possible.

## Start Here

- DSL: `docs/dev/dsl/overview.md`
- Foundations: `docs/dev/foundations/README.md`
- UI: `docs/dev/ui/overview.md`
- Technologies: `docs/dev/technologies/README.md`
- Platform maps: `docs/dev/platform/README.md`

## Common Paths

- UI foundations and system-wide contracts: `docs/dev/ui/foundations/README.md`
- Development foundations: `docs/dev/foundations/development/README.md`
- UI patterns and shared shell structures: `docs/dev/ui/patterns/README.md`
- System surfaces and delegated flows: `docs/dev/ui/system-experiences/README.md`
- Query-driven data surfaces: `docs/dev/ui/collections/README.md`
- DSL query posture: `docs/dev/dsl/db-queries.md`
- Network and distributed app features: `docs/dev/technologies/nexusnet-sdk.md`
- Family and managed-device posture: `docs/dev/technologies/family-mode.md`
- Developer workstation map: `docs/dev/platform/developer-workstation.md`
- Group/device management map: `docs/dev/platform/group-and-device-management.md`

## Rule Of Thumb

- If the question is about **how UI should look or behave**, start in `docs/dev/ui/`.
- If the question is about **how the DSL models something**, start in `docs/dev/dsl/`.
- If the question is about **how local development on Nexus is meant to work across surfaces**, start in
  `docs/dev/foundations/`.
- If the question is about **when to depend on a platform capability or substrate**, start in `docs/dev/technologies/`.
