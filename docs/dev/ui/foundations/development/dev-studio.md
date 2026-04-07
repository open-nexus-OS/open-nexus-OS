<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Dev Studio

Dev Studio is the first-party integrated development environment for Open Nexus OS.

Its role is not to invent a second platform inside the platform. Instead, it should bring together the same developer
surfaces the system already provides:

- Dev Mode,
- Console,
- Package Manager,
- local runtimes and tools,
- local services,
- and install/sideload workflows.

## Role in the platform

Dev Studio should prove that Open Nexus OS is not only a platform you can target, but also a platform you can
comfortably build on.

That means it should help developers:

- create and open projects,
- inspect issues and logs,
- install the runtimes and tools their project needs,
- run builds, tests, packs, and local deploy flows,
- and use the same canonical install and execution model as the rest of the system.

## What developers should expect

A developer using Dev Studio should be able to:

- open a project and see what toolchain it needs,
- install missing runtimes or tools,
- launch Console sessions in the right project context,
- start or inspect local services,
- run build/test/pack/install flows,
- and move from local work to sideload/store-facing workflows without switching mental models.

## Relationship to Console

Console and Dev Studio should complement each other:

- Console is the direct command-line and automation surface,
- Dev Studio is the guided, integrated workflow surface,
- and both should be backed by the same package manager, environments, and install model.

Developers should not have to guess whether the "real" workflow lives in one place and the "UI version" lives in another.

## Relationship to Package Manager

Dev Studio should rely on the platform's canonical install/update model for:

- runtimes,
- tools,
- local services,
- and app packaging/install workflows.

It should not create a parallel hidden toolchain store or a private package universe that behaves differently from the
rest of the system.

## Best practice

- treat Dev Studio as an orchestrator over the platform's real developer surfaces,
- keep Console and Dev Studio workflows aligned,
- make missing runtimes or services easy to understand and install,
- and preserve clear links between project state, installed tools, and local execution contexts.

## Avoid

- hiding critical developer state inside IDE-only abstractions,
- duplicating the package or install model just for the IDE,
- or turning Dev Studio into the only usable development path on the platform.

## Related docs

- `docs/dev/technologies/dev-mode.md`
- `docs/dev/ui/foundations/development/console.md`
- `docs/dev/ui/foundations/development/package-manager.md`
- `tasks/TRACK-DEVSTUDIO-IDE.md`
