# State quotas (`/state`, RFC-0072 amendment 2026-09-07)

Per-subject byte quotas on the app-writable state store. One model (TASK-0133)
enforced at one seam (statefsd `put`, TASK-0043); `/data` (nxfs) adopts the
same model after TASK-0317.

## Codes

| surface | code | name |
|---|---|---|
| statefs wire | `STATUS_QUOTA_EXCEEDED = 12` | `StatefsError::QuotaExceeded` |
| VFS (RFC-0072 table) | `14` | `EDQUOTA` (`VfsError::QuotaExceeded`) |

`EDQUOTA` is neither `ENOSPC` (provider full) nor `E2BIG` (one object over a
cap). A caller surface that cannot represent it maps to `ENOSPC` explicitly.

## Declaration (`policies/*.toml`)

```toml
[quota."selftest-client"]
prefixes   = ["/state/app/selftest/", "/state/selftest/"]   # ≤ 8 literal canonical prefixes
soft_bytes = 65536                                         # warn once per window
hard_bytes = 131072                                        # deny with EDQUOTA (≥ soft_bytes)
```

Attribution is by declared prefix set: `used = Σ (key_len + value_len)` over
the live keys under the subject's prefixes, reconstructed deterministically
from the journal at replay. Who may write under a prefix is the capability and
RFC-0091 profile question, decided before the quota is consulted.

## Enforcement (statefsd `put`)

- `next = used − old_len(key) + new_len`
- `next > hard_bytes` ⇒ `STATUS_QUOTA_EXCEEDED` before the journal append
  (nothing reaches the medium), audited (`AuditReason::QuotaExceeded`,
  `quota_denies_total{subject}`).
- `next > soft_bytes` ⇒ the put proceeds; `statefs: quota warn subject=<sid hex>
  used=<n> soft=<s>` once per subject per boot, re-armed once `used` drops below
  `soft_bytes`.
- `del` is never quota-denied. Subjects without a `[quota]` section and keys
  outside every declared prefix set are unmetered (opt-in, like profiles).

## Markers

- `statefs: quota warn subject=… used=… soft=…`
- `statefs: quota deny subject=… used=… hard=…`
- `SELFTEST: quota deny ok` (headless / smp1)

## Proof

- `cargo test -p statefs -p nexus-vfs-types` — code mapping,
  `test_reject_quota_code_is_distinct` (TASK-0043 P0, delivered).
- `tests/state_quota_host/` — `test_reject_write_over_hard_quota`, deterministic
  accounting across replay, soft-warn-once, delete frees (TASK-0043 P1).

Status: codes + contract delivered (P0); accounting/enforcement/markers follow
in TASK-0043 P1. This is a storage-surface guardrail; kernel-owned resource
truth is TASK-0286/0287.
