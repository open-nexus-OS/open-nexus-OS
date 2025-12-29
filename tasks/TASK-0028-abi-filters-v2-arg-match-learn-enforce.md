---
title: TASK-0028 ABI filters v2: argument matchers + learn→enforce + policy generator (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (ABI filter v1 dispatcher): tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md
  - Depends-on (audit/learn sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (policy authority): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0019 defines ABI syscall guardrails in `nexus-abi` (userland, kernel untouched). v2 extends that
system with:

- argument-level matching (path prefixes/regex, port ranges, size/deadline),
- a **learn** mode to record real call traces,
- an **enforce** mode to deny calls that don’t match,
- a small generator tool to produce conservative TOML policies from learn logs.

## Goal

Prove deterministically:

- Host: matcher precedence and learn→generate→enforce roundtrips.
- OS/QEMU (once v1 exists): selftest can switch learn→enforce and demonstrates allow+deny markers.

## Non-Goals

- Kernel-enforced syscall sandboxing (this remains a userland guardrail).
- Full “automatic policy” (generator emits a starting point; humans must review).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Deterministic matching and precedence.
- Bounded memory / bounded log emission:
  - learn logs must be rate-limited or sampled (avoid log spam),
  - generator must dedupe and cap rule explosion.

## Red flags / decision points

- **RED (gating)**:
  - v2 depends on v1 existing: without the central dispatcher/filter chain from TASK-0019, there is no
    single enforcement point and learn logs are incomplete.
- **YELLOW (policy authority drift)**:
  - Decide one authoritative source for profiles:
    - **Preferred**: `policyd` serves ABI profiles (single policy authority, audited).
    - Alternative: a dedicated `abi-filterd` loader service (only if policyd coupling is undesirable).
  - Do not ship two competing profile trees.
- **YELLOW (regex determinism)**:
  - Use a deterministic regex engine and keep patterns bounded. Prefer prefix rules over regex.

## Contract sources (single source of truth)

- ABI filter v1: TASK-0019
- Audit sink: TASK-0006
- Policy model: TASK-0008 (nexus-sel / policyd)

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests:

- v2 parser + matcher precedence:
  - deny beats allow when both match
  - longest-prefix wins
  - default deny unless fallback allow
- argument-level bounds:
  - size.max
  - deadline.max_ms
  - port allowlists and ranges
- learn→generate→enforce roundtrip:
  - produce TOML v2 from learn events
  - enforce against same trace yields allow (and a known forbidden op yields deny)

### Proof (OS / QEMU) — after TASK-0019 + TASK-0006

Extend `scripts/qemu-test.sh` (order tolerant):

- `abi-filterd: ready` (or `policyd: abi profiles ready` if policyd is the server)
- `SELFTEST: abi learn collected ok`
- `SELFTEST: abi enforce allow ok`
- `SELFTEST: abi enforce deny ok`

Notes:

- Postflight scripts must delegate to canonical tests/harness; no independent “log greps = success”.

## Touched paths (allowlist)

- `userspace/security/abi-policy/` (v2 schema + parser + matcher)
- `source/libs/nexus-abi/` (learn/enforce filters; argument extraction)
- `source/services/policyd/` and/or `source/services/abi-filterd/` (profile distribution + mode control)
- `tools/abi-gen/` (generator tool)
- `tests/` (host tests)
- `source/apps/selftest-client/` (OS markers)
- `docs/security/abi-filters.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. **Policy schema v2 + parser**
   - TOML v2 fields:
     - path allow/deny prefix lists
     - bounded regex allow list (optional)
     - port allowlist + range
     - size + deadline bounds
   - Deterministic precedence rules as tested.

2. **nexus-abi filter chain v2**
   - Extend the syscall model to expose matchable args:
     - `StatePutAtomic { path, size, deadline_ms }`
     - `NetBind { port }`, `NetConnect { dst_port }`
     - optional: VFS opens (RO/RW) if present
   - Add `LearnFilter`:
     - never denies
     - emits structured `abi.learn` events to logd (rate-limited/sampled)
   - Add `Enforce` mode:
     - denies mismatches with stable `EPERM`
     - emits audit deny events via logd.

3. **Profile distribution + mode switching**
   - If policyd is the authority: add `GetAbiProfile(subject)` and `SetAbiMode(subject, mode)` RPC.
   - Otherwise implement `abi-filterd` with the same surface.
   - Hot reload on profile updates; clients re-fetch.

4. **Generator tool (`abi-gen`)**
   - Input: `logd` learn events (JSONL) filtered by subject.
   - Output: conservative TOML v2 skeleton:
     - dedupe observed prefixes/ports
     - derive max size/deadline bounds from observed maxima
     - only include rules with ≥N samples.

5. **Selftest (OS)**
   - learn mode: run a small syscall trace and assert learn marker
   - enforce mode: run allowed trace and assert allow marker
   - enforce deny: run forbidden op and assert deny marker.

## Docs (English)

Update `docs/security/abi-filters.md`:

- v2 schema, precedence, determinism notes
- learn vs enforce semantics (and why learn is not a policy)
- generator workflow and best practices (tighten prefixes, avoid regex when possible).
