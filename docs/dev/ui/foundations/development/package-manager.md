<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Package Manager

The Package Manager is the developer-facing install and update surface for apps, runtimes, tools, and local services on
Open Nexus OS.

It should let developers work locally with familiar ideas like versioned installs, project environments, and automation,
without inheriting the platform-wide mutation and hidden side effects that often come with traditional package-manager
workflows.

## What it should manage

The platform should clearly distinguish between installable kinds such as:

- **app**
- **runtime**
- **tool**
- **service**

Examples:

- `python@3.13` as a runtime,
- `npm@10` as a tool,
- `postgres@17` as a local service,
- and regular end-user applications as apps.

## User and developer experience

From a developer point of view, the package manager should make it straightforward to:

- install a runtime for a project,
- update a tool without guessing which copy is active,
- add a local database or dev server,
- inspect what is installed and at which version,
- and understand whether something belongs to the user, the project, or a specific service context.

Representative examples:

```bash
nx pkg install runtime python@3.13
nx pkg install tool npm@10
nx pkg install service postgres@17
nx pkg list
```

These are product-facing examples, not a frozen command contract.

## Environments and scopes

The package manager should support a cleaner model than one machine-wide mutable tool pile.

Useful scopes include:

- **user scope** for tools and runtimes you generally want available,
- **project scope** for dependencies and versions tied to one workspace,
- and **service scope** for local services with their own data and lifecycle.

This helps the platform stay understandable for students and teams:

- the project knows which runtime it expects,
- local services are explicit,
- and updates or rollbacks are easier to reason about.

## Toolsets and network profiles

Over time, package-management flows may also expose higher-level concepts such as:

- **toolsets**: curated sets of tools or runtimes that belong together,
- **network profiles**: bounded package-resolution or install profiles,
- and **environment profiles**: reusable development setups for a language or stack.

These should make development easier without hiding important choices behind magic.

## Relationship to Console and Dev Studio

The Package Manager should work naturally with:

- **Console**, where many developers will run install/update and automation flows,
- **Dev Studio**, which should orchestrate the same canonical install model instead of inventing a second one,
- and **Dev Mode**, which makes these surfaces available when the device is intentionally being used for development.

## Best practice

- prefer managed installs and project environments over ad-hoc global mutation,
- make installed kinds visible and understandable,
- keep versioning and update behavior explicit,
- and treat local services as first-class installable things rather than background accidents.

## Avoid

- assuming every install belongs in one global user-wide bucket,
- treating package-management hooks as a license for arbitrary machine mutation,
- or designing developer installs so opaquely that students cannot tell what the system just changed.

## Related docs

- `docs/dev/technologies/dev-mode.md`
- `docs/dev/ui/foundations/development/console.md`
- `docs/dev/ui/foundations/development/dev-studio.md`
- `docs/packaging/artifact-kinds.md`
- `docs/security/shell-scripts-and-automation.md`
