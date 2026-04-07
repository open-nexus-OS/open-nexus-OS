<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Shell scripts and automation

Open Nexus OS should support shell scripts, package-manager automation, and developer setup flows without falling back to
legacy ambient-authority workstation behavior.

This page documents the security posture for that support.

## Why this matters

Developers realistically need:

- setup scripts,
- install automation,
- package-manager hooks,
- build pipelines,
- local dev-server startup,
- and service-oriented command-line workflows.

Refusing those workflows would make the platform feel closed and impractical.

Allowing them with unrestricted machine authority would erase the platform's security model.

## Core stance

The intended posture is:

- **scripts are allowed**
- **automation is expected**
- **compatibility matters**
- but execution should stay **bounded, explicit, and auditable**

The platform should prefer managed and brokered developer flows where possible, while still supporting compatibility
paths for ecosystems that rely on shell automation.

## Preferred model

Preferred developer automation should use:

- explicit package-manager and environment operations,
- project-scoped execution contexts,
- managed local services,
- and well-defined runtime/tool installs.

This gives developers practical automation without implying that every script can mutate the whole machine.

## Compatibility model

When traditional shell scripts or package-manager lifecycle hooks are needed, they should run under a compatibility
posture that is still bounded.

Important expectations:

- no silent platform-wide mutation by default,
- no unrestricted service registration,
- no broad filesystem access unless explicitly granted,
- no unbounded network or background behavior by default,
- and no implicit conversion from "script can run" to "script owns the machine".

## High-risk examples

These cases deserve special care:

- `npm` lifecycle scripts,
- `pip` / native-extension build flows,
- hand-written `install.sh` bootstrap scripts,
- compiler/linker invocations that want broad host access,
- local server startup scripts that want ports, storage, and background execution.

These are valid workflows, but they must not become a policy bypass path.

## Security expectations

Any shell/automation model should preserve:

- deny-by-default authority,
- bounded filesystem scope,
- bounded network scope,
- explicit service lifecycle rules,
- auditability of sensitive actions,
- and clear separation between developer convenience and production trust.

## Relationship to capabilities

Shell and automation support should remain aligned with the capability model:

- scripts should not magically bypass package, process, network, or service policy,
- command-line tools should still operate under explicit authority,
- and future developer-workstation capability families should describe these powers clearly.

See `docs/security/capabilities.md` for the current capability catalog and planned capability families.

## Relationship to packaging and installs

Developer automation is closely tied to packaging:

- apps, runtimes, tools, and services should remain distinguishable,
- install/update/remove flows should still use the platform's install authority,
- and automation outputs should be understandable as managed artifacts rather than mystery machine state.

## Best practice

- prefer project-aware automation over machine-wide mutation,
- make package-manager and script behavior visible enough that developers can understand what changed,
- keep shell support powerful enough for real work,
- and resist compatibility shortcuts that reintroduce Unix ambient authority under a different name.

## Avoid

- treating "developer shell" as equivalent to unrestricted admin shell,
- letting install hooks quietly register or persist global services,
- or making automation so locked down that common developer workflows become unrealistic.

## Related docs

- `docs/security/capabilities.md`
- `docs/dev/technologies/dev-mode.md`
- `docs/dev/foundations/development/console.md`
- `docs/dev/foundations/development/package-manager.md`
- `docs/packaging/artifact-kinds.md`
