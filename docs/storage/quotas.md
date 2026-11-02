---
schema: 1
title: "State quotas"
role: reference
subsystem: storage
sources: [userspace/statefs/src/quota.rs, source/services/statefsd/src/quota_os.rs, source/services/statefsd/build.rs, userspace/policy/src/schema.rs, tests/state_quota_host]
contracts: [RFC-0072]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# State quotas

> **Purpose** — how per-subject byte quotas on `/state` are declared, counted and enforced.
> **Owns** — `userspace/statefs/src/quota.rs`, `source/services/statefsd/src/quota_os.rs`
> **Contracts** — [RFC-0072](../rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md) (quota amendment: `EDQUOTA`)
> **Invariants** — One model, enforced at one seam (statefsd `put`). Usage is recomputed from the replayed map, never kept as a counter. `del` is never quota-denied.
> **Read next** — [statefs.md](statefs.md), [README.md](README.md)

## Codes

| surface | code | name |
|---|---|---|
| statefs wire | `STATUS_QUOTA_EXCEEDED = 12` | `StatefsError::QuotaExceeded` |
| VFS (RFC-0072 table) | `14` | `EDQUOTA` (`VfsError::QuotaExceeded`) |

`EDQUOTA` is neither `ENOSPC` (provider full) nor `E2BIG` (one object over a cap). A caller
surface that cannot represent it maps it to `ENOSPC` explicitly.

## Declaration (`policies/*.toml`)

```toml
[quota."selftest-client"]
prefixes   = ["/state/app/selftest/quota/"]   # ≤ 8 literal canonical directory prefixes
soft_bytes = 256                              # warn once
hard_bytes = 512                              # deny with EDQUOTA (≥ soft_bytes)
```

- Attribution by declared prefix set: `used = Σ (key_len + value_len)` over live keys under the
  subject's prefixes, reconstructed from the journal at replay.
- Prefix sets of different subjects must be disjoint (`check_quotas_disjoint`); ≤ 16 rules.
- Who may write under a prefix is the capability / RFC-0091 profile question, decided before the
  quota is consulted.
- Subjects without a `[quota]` section and keys outside every declared set are unmetered.
- `/data` (nxfs) has no quotas.

## Enforcement (statefsd `put`)

- `next = used − old_len(key) + new_len`
- `next > hard_bytes` ⇒ `STATUS_QUOTA_EXCEEDED` before the journal append (nothing reaches the
  medium); audited with `DenyReason::QuotaExceeded`, counted in `quota_denies_total`.
- `next > soft_bytes` ⇒ the put proceeds; one `statefs: quota warn …` per subject per boot,
  re-armed once `used` drops below `soft_bytes`.

## Markers

- `statefs: quota warn subject=0x… used=… soft=…`
- `statefs: quota deny subject=0x… used=… hard=…`
- `SELFTEST: quota deny ok`

## Implementation

| piece | where |
|---|---|
| model | `userspace/statefs/src/quota.rs` (`QuotaRule`, `check_put`, `WarnLatch`) |
| usage | `JournalEngine::used_under` / `stored_len` (`userspace/statefs/src/lib.rs`) — survives reopen, the block upgrade and compaction |
| grammar | `userspace/policy/src/schema.rs` (`compile_quota`, `check_quotas_disjoint`); fixtures `policies/tests/*quota*` |
| table | `source/services/statefsd/build.rs` bakes `QUOTA_ENTRIES` from `policies/*.toml`; an invalid or overlapping declaration fails the build |
| seam | `source/services/statefsd/src/quota_os.rs`, after the capability check and RFC-0091 evaluation, before the envelope check |

## Proof

- `cargo test -p nexus-wire` — `test_reject_quota_code_is_distinct` (code mapping).
- `tests/state_quota_host/` — `test_reject_write_over_hard_quota`, deterministic accounting across
  replay, soft-warn-once, delete frees.
- QEMU: `SELFTEST: quota deny ok`.

Kernel-owned resource limits are a separate layer (TASK-0286/0287).
