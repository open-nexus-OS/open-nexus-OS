---
title: TASK-0230 nx sec v1 (offline): security introspection CLI (`nx sec`) over policyd/sysfilter/sandbox/quotas + deterministic tests/markers
status: Draft
owner: @security @devx
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DevX CLI base (`nx`): tasks/TASK-0045-devx-nx-cli-v1.md
  - Policy as Code (`nx policy ...`): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Policy snapshot artifact (optional fast path): tasks/TASK-0229-policy-snapshot-v1-canonical-json-bin-sysfilter-profiles-quotas.md
  - Kernel sysfilter (true syscall enforcement): tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md
  - Sandbox profiles (IPC/VFS allowlists): tasks/TASK-0189-sandbox-profiles-v2-sandboxd-or-policyd-distribution-ipc-vfs.md
  - ABI filters (guardrails): tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md
  - Quotas / egress (userspace): tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We already have plans for:

- policy tooling (`nx policy`) via Policy as Code (`TASK-0047`),
- true syscall enforcement in kernel (`TASK-0188`),
- sandbox profiles for IPC/VFS (`TASK-0189`),
- quotas/egress rules (`TASK-0043`).

What’s missing is a cohesive operator/dev CLI surface to:

- inspect the *effective* security posture of a running system,
- run deterministic negative tests (deny/quota) without inventing ad-hoc scripts,
- export bounded, offline diagnostics for security triage.

This task adds `nx sec` as a subcommand of the canonical `nx` CLI (no separate binary).

## Goal

Deliver `nx sec` commands that work offline and deterministically:

- `nx sec policy show` (delegates to `nx policy` / policy snapshot if present)
- `nx sec ps` (list subjects with: identity, profile id, syscall mask summary, sandbox profile name)
- `nx sec syscalls --subject <id>` (show allowlisted syscalls)
- `nx sec sandbox --subject <id>` (show IPC/VFS allowlists)
- `nx sec quotas --subject <id>` (show configured quotas; show current usage only if a real counter source exists)
- `nx sec test deny --subject <id> --syscall <name>` (deterministic deny proof)
- `nx sec test quota ...` (deterministic quota trigger against a fixture service/app; no timing flukes)

## Non-Goals

- Creating a new policy compiler format or separate “security_hardening_v1.json” schema.
- Network upload or remote management.
- Claiming “current RSS/handles/pages” usage unless there is a real kernel/userspace counter ABI (must be gated and explicit).

## Constraints / invariants (hard requirements)

- Offline-only, deterministic output ordering.
- No fake success: `nx sec test ... ok` markers only after the deny/quota behavior is actually observed.
- No new URI schemes; reuse existing conventions.
- Subcommand lives under the existing `nx` toolchain (avoid `nx-sec` tool drift).

## Red flags / decision points

- **RED (introspection data sources)**:
  - If kernel sysfilter is not enabled (`TASK-0188` not landed), `nx sec syscalls` must clearly report “unsupported”
    instead of making up a mask.
  - If sandbox profile distribution is not present, `nx sec sandbox` must be “unsupported”.
- **YELLOW (authority drift)**:
  - Prefer querying `policyd` (or the single chosen profile server) rather than reading random files.
  - If `policy.bin` snapshot exists, treat it as an artifact of the policy system, not a second source of truth.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p nx_sec_host -- --nocapture` (new):
  - fixture policy snapshot produces stable CLI output (golden),
  - deny tests against a fixture “syscall probe” return deterministic error codes (simulated host harness),
  - output ordering stable.

### Proof (OS/QEMU) — gated

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
- Markers:
  - `SELFTEST: sec syscall deny ok` (ties to `TASK-0188` sysfilter proof)
  - `SELFTEST: sec profile windowd ok` (ties to `TASK-0189`)
  - `SELFTEST: sec quota ipc ok` (ties to kernel IPC budget selftests + quotas tasks; only if real)

## Touched paths (allowlist)

- `tools/nx/` (add `sec` subcommand)
- `tests/nx_sec_host/` (new)
- `source/apps/selftest-client/` (invoke deny/quota fixtures; markers)
- `docs/security/` (new `nx-sec.md` usage)

## Plan (small PRs)

1. Define the CLI UX and stable output formats (host-first).
2. Implement host fixtures for deterministic golden outputs.
3. Add OS wiring once data sources exist (sysfilter + sandbox profiles + quota sources), then add QEMU markers.

## Acceptance criteria (behavioral)

- `nx sec` provides a deterministic, offline view of effective security posture and can trigger real deny paths under QEMU without inventing a parallel tooling ecosystem.
