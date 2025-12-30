---
title: TASK-0229 Policy snapshot v1 (host-first): canonical policy → stable JSON + compact binary snapshot for OS (sysfilter profiles, quotas, sandbox)
status: Draft
owner: @security @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Policy as Code (unified tree + canonicalization): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config distribution + 2PC apply: tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Kernel sysfilter (TaskProfileId + syscall allowlist): tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md
  - Sandbox profiles (userspace IPC/VFS rules): tasks/TASK-0189-sandbox-profiles-v2-sandboxd-or-policyd-distribution-ipc-vfs.md
  - Quotas / egress (userspace enforcement): tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md
---

## Context

The security hardening prompts regularly propose a separate “policy compiler” (YAML → JSON → bin).
Repo direction already defines **Policy as Code** as the single authority and canonicalization source (`TASK-0047`),
and we must avoid format/authority drift.

However, OS builds benefit from a **compact, fast-to-load snapshot**:

- stable lookups by `service_id`/profile id,
- stable syscall masks for kernel sysfilter,
- stable quotas and sandbox allowlists,
- deterministic proof artifacts for CI.

This task adds a snapshot compiler **as an output of the existing policy tree**, not as a new parallel system.

## Goal

Deliver a host-first snapshot compiler that, given the canonical policy tree:

- produces **canonical JSON** (`policy.json`) with stable ordering (already required by `TASK-0047`), and
- produces a **compact binary snapshot** (`policy.bin`) suitable for OS consumption:
  - stable, versioned layout,
  - varint/bitset tables for syscall masks and profile mappings,
  - bounded decode in no_std.

## Non-Goals

- Introducing YAML as the new source of truth. Authoring format stays as defined by `TASK-0047` (currently TOML tree).
- Introducing a second policy authority service. `policyd` remains the single decision point.
- Making kernel interpret complex policy rules. Kernel should consume **small, precomputed** items only (e.g., syscall bitmasks).

## Constraints / invariants (hard requirements)

- **Determinism**:
  - snapshot outputs must be byte-identical given identical inputs;
  - no timestamps or host paths embedded;
  - stable string normalization rules.
- **Versioned format**:
  - `policy.bin` must start with a magic + version and include a tree hash for traceability.
- **Bounded decoding**:
  - OS decode must reject oversize tables deterministically with stable errors.
- **Single identity model**:
  - mapping keys are kernel-derived identities (`service_id`) and/or `TaskProfileId` (see `TASK-0188`).

## Red flags / decision points

- **RED (profile identity join)**:
  - We must decide the join key between userspace profiles and kernel sysfilter:
    - either `TaskProfileId` is assigned by the spawner from a policy table,
    - or kernel derives it from `service_id` via a table passed at boot.
  - This task should not invent an implicit, unversioned mapping.
- **YELLOW (policy surface creep)**:
  - `policy.bin` must carry only what OS needs for fast enforcement (syscall mask tables, quotas, allowlists),
    not the full evaluator logic.

## Contract sources (single source of truth)

- Canonical policy tree + canonical JSON rules: `TASK-0047`
- Kernel sysfilter profile contract: `TASK-0188`

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p policy_snapshot_host -- --nocapture` (new):
  - compiling fixture policy tree produces:
    - `policy.json` stable bytes,
    - `policy.bin` stable bytes (golden hash),
    - and a verified `tree_sha256` that matches `TASK-0047`’s policy version.
  - decoding `policy.bin` round-trips to the same logical tables (stable).
  - negative tests reject oversize tables and invalid versions deterministically.

### Proof (OS/QEMU) — gated

Once `policyd` + sysfilter + profile application are real:

- QEMU markers proving the kernel received a sysfilter profile and applied it (`TASK-0188` markers).

## Snapshot content (v1 minimum)

- **Header**: magic, version, `tree_sha256[32]`.
- **Service/profile mapping**:
  - `service_id -> profile_id` (or `exe_name -> profile_id` only if identity is provenance-safe).
- **Syscall masks**:
  - `profile_id -> syscall_bitset` (fixed width; includes cap_* breakdown if needed).
- **Rate bucket parameters** (optional for v1; if included must be bounded).
- **Quotas**:
  - `profile_id -> { pages?, handles?, ipc_bytes? }` (only if the kernel/userspace enforcement points exist).
- **Sandbox allowlists** (if used by samgr/vfsd):
  - IPC allowlist patterns (compiled),
  - VFS prefix allowlists (compiled).

## Touched paths (allowlist)

- `tools/` (new: snapshot compiler tool; must be integrated with existing `nx policy` tooling, not parallel)
- `userspace/security/` (new: snapshot encode/decode crate, no_std-friendly)
- `tests/` (new host tests + fixtures)
- `docs/security/policy-as-code.md` (extend: snapshot artifacts + versioning)

## Plan (small PRs)

1. Define binary layout + decoder constraints + fixtures.
2. Implement compile + verify + roundtrip host tests.
3. Wire into `nx policy` tooling as `nx policy snapshot build/verify` (or equivalent, per `TASK-0047`).
4. Document how sysfilter/sandbox consumers read the snapshot (gated; no OS markers unless wired).

## Acceptance criteria (behavioral)

- A single policy tree produces deterministic snapshot artifacts that are safe to decode in OS builds and can feed kernel sysfilter/profile application without introducing a new policy authority or file format drift.
