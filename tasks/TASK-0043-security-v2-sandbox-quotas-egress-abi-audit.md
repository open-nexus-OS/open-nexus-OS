---
title: TASK-0043 Security v2: sandbox quotas (tmp/state) + per-subject network egress rules + tighter ABI policies + audits (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (sandboxing v1): tasks/TASK-0039-sandboxing-v1-vfs-namespaces-capfd-manifest.md
  - Depends-on (ABI filters v2): tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md
  - Depends-on (policy authority): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (OS networking surface): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

After Sandboxing v1 exists (namespaces + CapFd + manifest-driven views), we want stronger isolation:

- per-app quotas for tmp/state write paths (deny-on-exceed),
- per-subject network egress rules (CIDR/ports, default deny),
- tighter ABI policy matching and auditable deny reasons.

Repo reality today:

- OS-lite VFS is still read-only `pkg:/` only; namespaces/state/tmp are not real on OS yet.
- OS networking and sockets surface are planned tasks.

So this task is **host-first** and **OS-gated**.

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
