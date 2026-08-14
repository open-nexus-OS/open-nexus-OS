---
title: TASK-0043 Security v2: sandbox quotas (tmp/state) + per-subject network egress rules + tighter ABI policies + audits (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0003
  - TASK-0006
  - TASK-0008
  - TASK-0028
  - TASK-0039
follow-up-tasks:
  - TASK-0133
  - TASK-0188
  - TASK-0286
  - TASK-0287
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Depends-on (sandboxing v1): tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md
  - Depends-on (ABI filters v2): tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md
  - Depends-on (policy authority): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (OS networking surface): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Testing contract: scripts/qemu-test.sh
---

## Rebase 2026-08-14 (prerequisites landed; quotas/egress genuinely unbuilt)

Verified against the repo on 2026-08-14. The old "Repo reality today" claims below were false and are
corrected. **Do NOT re-implement** the prerequisites; they shipped:

- **Sandboxing v1 shipped (TASK-0039, Done).** vfsd carries namespace view enforcement, canonical
  path resolution/traversal rejection, and HMAC-integrity CapFd tokens with replay-guarded
  verification (`source/services/vfsd/src/sandbox.rs:4-6` CONTEXT header; implementation in that
  module). VFS is NOT "read-only `pkg:/` only" anymore.
- **Persistence shipped (TASK-0009, Done).** `/state` via statefsd is real.
- **OS networking shipped (TASK-0003, Done for Track A/B).** Networking is not "planned"; the
  substrate exists (Noise XK follow-up tracked separately in TASK-0003B).

**Genuinely unbuilt** (checked: zero quota/EDQUOTA code in `source/services/vfsd/` or
`source/services/statefsd/`; zero egress/CIDR policy code anywhere in `source/`):

- per-subject quotas for `/tmp` + `/state` write paths (deny-on-exceed),
- per-subject egress policy (CIDR/ports, default deny),
- the ABI audit trail tightening (structured deny reasons + counters).

That is the honest residual scope of this task. Kernel untouched.

> **Coordination note (binding): ONE quota model, shared with TASK-0133.**
> The quota model here MUST be the same model as TASK-0133 (statefs quotas, `/state`):
> this task **adopts** the TASK-0133 accounting/enforcement model — `EDQUOTA` / soft-warn /
> hard-deny semantics — and never forks a second quota model. TASK-0133's ledger already carries
> the matching note pointing the `/data` (nxfs) half at the storage ladder
> (seed on the same model after TASK-0317). If this task lands first, its implementation becomes
> the reference implementation of that shared model, not a competing one.

## Context

After Sandboxing v1 exists (namespaces + CapFd + manifest-driven views), we want stronger isolation:

- per-app quotas for tmp/state write paths (deny-on-exceed),
- per-subject network egress rules (CIDR/ports, default deny),
- tighter ABI policy matching and auditable deny reasons.

Repo reality (corrected 2026-08-14, see rebase above): namespaces, `/state`, and OS networking all
exist; quotas and egress do not. The host-first / OS-gated split below still applies to the residual
scope.

## Goal

Prove deterministically on host that:

- quotas are enforced per subject and produce stable `EDQUOTA` denies,
- egress policy enforcement denies non-matching connect/bind attempts with `EPERM`,
- ABI policy rules can match these predicates and emit consistent audit reasons,
- deny events are exported to the audit sink once available.

Once OS prerequisites exist, add QEMU selftest markers.

## Non-Goals

- Kernel-enforced sandboxing (no kernel changes in v2).
- Full traffic shaping / bandwidth scheduling.
- Inbound firewalling (egress only).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic behavior: quotas and policy matching must not depend on wall-clock jitter.
- Bounded memory: per-subject counters and tables are bounded; denies are rate-limited.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: OS markers only once namespaces + net wrappers are truly used by apps.

## Red flags / decision points

- **RED (security boundary honesty)**:
  - Quotas and egress rules are **userspace enforcement**. They protect only if:
    - apps do not hold direct caps to bypassing services, and
    - network access is mediated by a controlled surface (`nexus-abi`/`nexus-net` wrappers or a net broker).
  - If apps can execute raw syscalls or talk directly to device services, kernel enforcement is required.
- **YELLOW (policy authority drift)**:
  - Avoid splitting logic across `vfsd`, ABI filters, and policyd. Prefer:
    - policyd/nexus-sel as the policy source of truth,
    - ABI filters as guardrails,
    - vfsd as the enforcement point for namespace+CapFd+quotas.

## Production-grade gate note

This task closes a strong **userspace sandboxing and quota floor**, but it is still not the full
release-grade resource/security boundary.

- `TASK-0133` tightens deterministic `/state` quota semantics.
- `TASK-0188` is the kernel-side syscall boundary follow-up.
- `TASK-0286` / `TASK-0287` add kernel-owned accounting truth and hard pressure enforcement.

Until those land, this task should be described as a host-first / mediated enforcement layer, not as complete kernel-backed isolation.

## Contract sources (single source of truth)

- Sandbox v1 contract: TASK-0039
- ABI filters v2: TASK-0028
- Audit sink contract: TASK-0006

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (`tests/security_v2_host/`):

- quotas:
  - write until limit exceeded → `EDQUOTA` and deny event recorded
- egress:
  - connect to disallowed CIDR/port → `EPERM` and deny event recorded
  - connect to allowed CIDR/port → allowed
- ABI tightening:
  - deny write when remaining budget insufficient (stable reason)
  - learn→enforce can capture egress attempts (if learn mode available).

### Proof (OS / QEMU) — gated

Once sandbox v1 namespaces, `/state`, and OS net wrappers exist:

- `vfsd: quota set (subject=<id> tmp=... state=...)`
- `vfsd: quota deny (subject=<id> ...)`
- `net-egress: enforced`
- `SELFTEST: quota deny ok`
- `SELFTEST: egress deny ok`
- `SELFTEST: egress allow ok` (if allowed in recipe)

## Touched paths (allowlist)

- `source/services/vfsd/` (quota controller in namespaces; host-first)
- `source/services/execd/` (apply quotas/policy at spawn; OS-gated)
- `source/libs/nexus-abi/` and/or `userspace/net/nexus-net/` (egress enforcement wrappers; gated)
- `userspace/security/` (new `net-egress` policy parser/matcher)
- `recipes/security/{quotas.toml,egress.toml}` (new)
- `tests/`
- `docs/security/sandboxing.md`
- `docs/security/network-egress.md` (new)
- `docs/security/abi-filters.md`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **Quota config + enforcement (vfsd)**
   - Add `recipes/security/quotas.toml` describing tmp/state budgets by subject or domain.
   - vfsd namespace tracks usage and denies writes with `EDQUOTA` on exceed.
   - Emit a deterministic marker on first quota set and first deny.

2. **Egress policy (userspace guardrail)**
   - Add `recipes/security/egress.toml` with default deny and per-subject allow rules (CIDR:ports).
   - Enforce in the controlled network surface (prefer: `nexus-net` connect/bind wrappers).
   - Marker on first enforcement: `net-egress: enforced`.

3. **ABI policy tightening**
   - Extend ABI filter v2 matching with:
     - egress predicates (dst CIDR/port),
     - quota-aware state writes (deny if remaining budget < requested).
   - Ensure all denies emit stable, structured audit reasons.

4. **Audit + metrics**
   - Denies emit structured audit records via logd when available; otherwise use a bounded test sink.
   - Expose counters:
     - `quota_denies_total{subject}`
     - `egress_denies_total{subject}`
     - `egress_allows_total{subject}` (optional).

5. **Docs**
   - Update sandboxing docs with quotas and error codes.
   - Add network egress policy doc with examples and audit expectations.
