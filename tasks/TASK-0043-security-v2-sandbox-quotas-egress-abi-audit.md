---
title: TASK-0043 Security v2: sandbox quotas (tmp/state) + per-subject network egress rules + tighter ABI policies + audits (host-first, OS-gated)
status: In Progress (P0–P3 delivered 2026-09-07)
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

## End-state rewrite 2026-09-03 (binding; supersedes older sections where they differ)

Verified repo reality (Explore 2026-09-03): zero quota/`EDQUOTA` code anywhere; the only
„budget“ in statefsd is a latency budget (`WriteBudget`, not bytes); `/tmp` does not exist —
`validate_namespace_path` accepts only `pkg:/`; statefs status codes end at 11
(`userspace/statefs/src/protocol.rs`), `VfsError` at 13 (RFC-0072 table, TASK-0132 reserves
`EDQUOTA`); netstackd `handlers/connect.rs` performs no authorization, the facade receives the
kernel `sid` and drops it (`FacadeContext` has no sender), `wire.rs` has no `STATUS_DENY`;
`NetBind` matches port ranges only, `NetConnect` does not exist; metrics counter substrate exists,
no `*_denies_total`; policyd `AuditReason` has one variant; `userspace/security/` and
`recipes/security/` do not exist. RED (real): `policies/base.toml` grants `selftest-client`
`device.mmio.net` and it runs its own network stack on the NIC — any such grant bypasses
netstackd enforcement; the grant distribution IS the boundary and is documented as such.

### Goal (end state)

Per-subject resource and network policy enforced at the real seams and audited with one deny
taxonomy: byte quotas on the app-writable state store (soft warn / hard deny with a stable
`EDQUOTA`), per-subject egress policy (CIDR/port allow-lists, default deny) decided by policyd at
the netstackd facade with kernel-attributed identity, structured deny reasons + bounded counters
for every deny.

### Non-goals

Kernel changes; traffic shaping; inbound firewalling (TASK-0052); a `/tmp` provider (app-private
scratch namespaces are TASK-0189 `vfs` territory); `/data` (nxfs) quotas before TASK-0317 — seeded
on the same model afterwards (TASK-0133 lineage), never a second model.

### Decisions

- **Quota model = TASK-0133 model, enforcement point = statefsd.** Per-subject soft + hard bytes,
  deterministic bounded accounting, hard limit ⇒ `EDQUOTA`, soft limit ⇒ one warn marker per
  subject per window. statefsd already holds the policyd check (`policy_allows`) and the subject
  canonicalization; a vfsd namespace quota would be a second model. TASK-0133 is marked „executed
  by TASK-0043 for `/state`“; its `/data` half stays with the storage ladder.
- **Wire**: statefs `STATUS_QUOTA_EXCEEDED = 12`, `VfsError::QuotaExceeded = 14` via an RFC-0072
  amendment (TASK-0132 reservation honoured). Markers `statefs: quota warn subject=<id> used=…
  soft=…`, `statefs: quota deny subject=<id> used=… hard=…`.
- **Shared network prerequisite (P2 = TASK-0052 P1)**: netstackd facade gets the sender (`sid` →
  `FacadeContext`), a `STATUS_DENY` wire code, and the `nexus_ipc::policyd::authorize` pattern
  (greppable `!cap-deny` marker) at connect / listen / udp-bind — ONE identity model in netstackd.
- **Egress rules** = the `net.connect` section of the TASK-0028 profile schema v2 (CIDR + port),
  evaluated by policyd; default deny for every subject without rules.
- **Audit taxonomy**: `AuditReason` += `QuotaExceeded`, `EgressDenied`, `IngressDenied`,
  `AbiRuleDenied{class}`; counters `quota_denies_total{subject}`, `egress_denies_total{subject}`
  (cardinality-capped under metricsd `max_series_total`); deny audits inherit logd's persisted
  evidence class (loop hazard respected).

### P0 delivered 2026-09-07 — RFC-0072 amendment: `EDQUOTA` codes + quota contract

- RFC-0072 table row `14 EDQUOTA` (the TASK-0132 reservation) + amendment section „per-subject
  byte quotas on `/state`“: codes (statefs `STATUS_QUOTA_EXCEEDED = 12` / `StatefsError::
  QuotaExceeded`, VFS `VfsError::QuotaExceeded = 14`), declaration `[quota."<subject>"]
  {prefixes ≤ 8, soft_bytes, hard_bytes}` in the policy SSOT (shared grammar), attribution by
  declared prefix set (deterministic from the journal, no new persisted field), enforcement rule
  (`next = used − old + new`, hard ⇒ deny before append, soft ⇒ warn once per boot, `del`
  never denied, opt-in per subject), markers, proof list; checklist Phase 4a ✅ / 4b (P1).
- Code (userspace, no approval zone): `userspace/statefs` status + error + mappings + envelope
  test; `userspace/vfs-types` `QuotaExceeded = 14` `EDQUOTA` + `from_code` + roundtrip +
  `test_reject_quota_code_is_distinct`; consumers `statefsd/emit_os.rs` (`statefsd: err quota`),
  `keystored/os_stub.rs` (maps to its size-class refusal). Docs: `docs/storage/quotas.md` (new),
  `docs/storage/statefs.md` appended-status note.
- ⭐ Harness finding (fixed here, policyd): TASK-0028 P3 started reading v2 replies at the right
  status offset, so every delegated cap check (one per statefs put) became an audit append —
  25 → 128 per boot, the per-boot cap exhausted, and each append waited up to 500 ms on logd's
  full queue (icount), stalling policyd long enough for the seams' 500 ms cap checks to time out
  into fail-closed denials (`SELFTEST: statefs auth put/tamper deny/rollback deny/v2 crash-atomic
  FAIL`, lane timeout). Fix: logd append send budget 500 ms → 2 ms (best-effort by contract,
  deferred stays counted/visible) and hot-path ALLOWs (`OP_CHECK_CAP_DELEGATED`, `OP_ABI_EVAL`)
  are not audited — every DENY and the mode switch are. smp1 back to 31 audits/boot, probes ok.
- Proof: `cargo test -p statefs -p nexus-vfs-types` green; `just check` green; `just test-all`
  green 2026-09-07 (`exit=0`, all nine QEMU lanes, after the policyd audit-path fix).
- Next: P1 (accounting in `userspace/statefs`, enforcement at statefsd put, `[quota]` in
  `schema.rs` + policyd table, `tests/state_quota_host/`, `SELFTEST: quota deny ok`).

### P1 delivered 2026-09-07 — statefs quota accounting + enforcement

- **Model** `userspace/statefs/src/quota.rs`: `QuotaRule {subject, prefixes ≤ 8, soft, hard}`,
  `entry_bytes = key_len + stored_len`, `rule_for` (by prefix), `check_put` (`next = used − old +
  new`, saturating; `> hard` ⇒ Deny, `> soft` ⇒ Warn), `WarnLatch` (once per window, re-armed
  below soft). Engine: `JournalEngine::stored_len`, `used_under(prefixes)` — usage is recomputed
  from the replayed map on every metered put (no cached counter ⇒ nothing drifts across reopen,
  the virtio upgrade or compaction; O(keys) per metered put, keys are few).
- **Declaration**: `schema.rs` `RawQuota`/`Quota`, `compile_quota` (1..=8 canonical directory
  prefixes, `0 < soft ≤ hard`), `check_quotas_disjoint` (no prefix under another subject's —
  attribution must be unambiguous), errors `QuotaNoPrefixes|QuotaTooManyPrefixes|QuotaLimits|
  QuotaOverlap|TooManyQuotas`; host `PolicyDoc::quota`, section allowlist; corpus `ok_quota` +
  5 `reject_quota_*`; policyd build.rs accepts the section (statefsd owns the table).
- **Table + seam**: statefsd `build.rs` (new; shared grammar + FNV by `#[path]`) → `QUOTA_ENTRIES`;
  `quota_os.rs` `QuotaState::admit_put` after cap check + RFC-0091 eval, before envelope/journal
  ⇒ `STATUS_QUOTA_EXCEEDED` + `statefs: quota deny subject=0x<sid> used=<n> hard=<h>` (audited)
  / `statefs: quota warn … soft=<s>` once per boot; `note_delete` re-arms. `policies/base.toml`
  `[quota."selftest-client"]` prefix `/state/app/selftest/quota/` soft 256 / hard 512.
- **Proofs**: `tests/state_quota_host/` (workspace member): `test_reject_write_over_hard_quota`
  (EDQUOTA = 12, nothing reaches the journal, exact-limit allowed), `accounting_is_deterministic_
  across_replay` (reopen + fresh engine agree), `soft_warn_once_per_window_and_rearm`,
  `delete_frees_and_overwrite_counts_delta`, `unmetered_prefixes_are_untouched`,
  `shipped_selftest_quota_matches_the_os_proof`. Selftest `statefs_quota_probe` (routing phase,
  after persist): 2 × 207 B fit (second crosses soft), third ⇒ `STATUS_QUOTA_EXCEEDED`, delete
  frees, same put succeeds ⇒ `SELFTEST: quota deny ok`; ladder gets `statefs: quota warn/deny
  subject=0x52c6…` + the marker (2-space list; proof-manifest `markers/routing.toml`).
- Proof: `cargo test -p state_quota_host -p statefs -p policy` green; OS-target strict check
  statefsd + selftest-client; `just check` green; `just test-all` green 2026-09-07 (`exit=0`, all
  nine QEMU lanes; `statefs: quota warn used=414 soft=256` / `quota deny used=414 hard=512` /
  `SELFTEST: quota deny ok` observed in every lane, the second warn after the delete shows the
  re-armed latch).
- Next: P2 (netstackd identity + `STATUS_DENY`, shared with TASK-0052 P1).

### P2 delivered 2026-09-07 — netstackd identity + `STATUS_DENY` (shared with TASK-0052 P1)

- **Identity**: the facade loop already received the kernel sender (`ipc_recv_v2` → `sid`) and
  dropped it; `FacadeContext.sender_service_id` now carries it to every handler (never a payload
  field). `STATUS_DENY = 6` appended to the netstackd wire.
- **Seam** `source/services/netstackd/src/os/facade/authz.rs`: `Seam::of(ctx)` copies the inputs
  before a handler's mutable borrows; `admit_connect(ip, port)` ⇒ `OP_ABI_EVAL` `net.connect`,
  `admit_bind(ip, port)` ⇒ `net.bind` with the address class (`loopback` = 127/8 or the facade's
  loopback emulation — QEMU user-net fallback IP / 0.0.0.0 on the loopback port set, traffic that
  never reaches the NIC; `any` otherwise). policyd is reached over init-wired FIXED slots
  (`wiring.rs` netstackd arm: 7 = policyd request SEND, 8/9 = own `@reply` RECV/SEND —
  `init: netstackd policy slots 7/8/9`), the statefsd/keystored pattern: an enforcement seam
  never routes dynamically from its hot loop. (A routed lookup from the facade start wedged
  netstackd under icount — `route_with_nonce_budgeted(policyd, 1, 2)` never returned although
  the same call works from abilitymgr; cause not chased, the fixed-slot design is the end state
  anyway. Tracked as a note for the reliability lane.) Boot witness written raw:
  `net-egress: enforced (netstackd policy seam on)`. Refusal ⇒ `STATUS_DENY` reply + `!cap-deny: enforcer=netstackd class=…
  port=… addr=… subject=0x…`. Hooked at connect (before dial), listen and udp bind (before the
  loopback/NIC split). `policies/base.toml`: netstackd += `policy.delegate` (it names the subject
  it serves, like statefsd).
- **Decision** `nexus_ipc::policyd::seam_admits(sender, status)`: unattributed (`sid == 0`) never
  admitted; `STATUS_ALLOW` and `STATUS_UNSUPPORTED` (governed = authored: no profile ⇒
  capability-only) admit; deny / unreachable / anything else refuse. `resolve_policy_slots()`
  (policyd + `@reply` routing) and `PolicySlots` added for services without fixed policyd slots.
  Host proof `test_reject_unattributed_connect` (nexus-ipc).
- **Live effect**: dsoftbusd (no profile) keeps working (UNSUPPORTED ⇒ admitted); the selftest's
  own UDP bind (port 34569 on 0.0.0.0 = loopback emulation, ≥ 1024) is allowed by its RFC-0091
  profile — the connect-class rules (`10.0.2.0/24` 53/80/443) become enforceable in P3.
- ⭐⭐ Build finding (fixed here, `scripts/build.sh`): the volume-service loop built a service
  ONLY when its release ELF was missing (`[[ ! -f "$elf_path" ]]`), so every volume service
  (metricsd, settingsd, timed, abilitymgr, sessiond, netstackd, dsoftbusd, hidrawd, touchd,
  gpud, windowd, inputd, imed, pinched) had been shipping its FIRST build since 2026-09-03/04 —
  the first P2 QEMU run never contained the new netstackd code (its payload had no seam strings).
  Every OS proof since TASK-0321 P4 ran stale volume services against fresh embedded ones; the
  additive wire contracts kept them interoperating, which is why nothing was noticed. Fix: cargo
  always runs for volume services (incremental no-op when unchanged); `NEXUS_SKIP_BUILD=1` stays
  the explicit escape. ⤳ Follow-up: a build gate that compares each bundle payload's build id
  against the source tree (TASK-0321 lineage).
- First-boot finding (before the build fix was known): dsoftbusd's connect retry storm (thousands of loopback connects per
  boot, folded `dbg:netstackd: connect req count 4096`) now cost one policyd roundtrip each and
  pushed the icount boot past the 200 s lane budget (`init: supervision persist restarts` missing).
  Fix: a bounded per-boot admit cache (`AdmitCache`, 16-entry ring of admitted (sender, class,
  addr_class, port, addr) tuples — profiles are static per boot; refusals are never cached so
  policyd audits/learns every one). The seam status is written raw (`nexus_abi::debug_write`
  from a stack buffer) because a service's routine `debug_println` lines fold even in proof
  boots.
- Proof: `cargo test -p nexus-ipc` (8 policyd tests), `cargo test -p netstackd` (host: wire
  vocabulary + deny reply with nonce), OS-target strict check netstackd + dsoftbusd,
  `just check` green, `just test-all` green 2026-09-07 (`exit=0`, all nine QEMU lanes,
  `net-egress: enforced (netstackd policy seam on)` in every boot, no `!cap-deny`). Ladder (both `init: netstackd policy slots 7/8/9`
  and `net-egress: enforced (netstackd policy seam on)`, 2-space list) — the seam's boot witness;
  `SELFTEST: egress deny/allow ok` are P3, `SELFTEST: ingress deny ok` is TASK-0052 P1.
- Next: P3 (egress proof: selftest connect to an allowed and a refused target over the facade,
  `net-egress: enforced`, `SELFTEST: egress deny ok` / `allow ok`, learn mode records the attempt).

### P3 delivered 2026-09-07 — egress policy proven through the facade

- **OS proof** `source/apps/selftest-client/src/os_lite/net/egress.rs` (net phase, single-VM):
  `connect` requests through netstackd against the shipped `net.connect` profile
  (`10.0.2.0/24` 53/80/443): `192.168.1.1:80` (CIDR) and `10.0.2.2:8080` (port) answer the
  seam's `STATUS_DENY` (never dialled; `!cap-deny: enforcer=netstackd class=net.connect
  dst=<a.b.c.d>:<p> subject=0x…` — the deny line now names the destination for connect, port +
  address class for bind) ⇒ `SELFTEST: egress deny ok`; `10.0.2.2:53` is admitted (any non-deny
  status — the dial outcome belongs to the network) ⇒ `SELFTEST: egress allow ok`; under Learn
  mode (`OP_SET_ABI_MODE` by the selftest, epoch-bound) a refused connect is admitted by
  policyd's collector (`OP_ABI_LEARN_STATS` +1) with the decision unchanged ⇒ `SELFTEST: egress
  learn collected ok` (refusals are never cached by netstackd, so policyd sees each one). Ladder:
  the two `!cap-deny` lines + the three markers (2-space list); proof-manifest `markers/net.toml`.
- **Host proofs** `tests/security_v2_host/` (workspace member): the shipped profile compiled
  through the shared grammar and evaluated by the real matcher, composed with `seam_admits`:
  `test_reject_egress_cidr`, `test_reject_egress_port`, `egress_allowed_target_passes`,
  `test_reject_unattributed_connect`, `default_deny_without_connect_rules`.
- ⭐⭐ Finding (fixed here, init `wiring.rs`): netstackd's facade serves FIXED slots recv 5 /
  send 6, but its server pair was delivered at the child's next free slots (3/4 since the
  volume spawn; the NIC MMIO cap sits at 48). Every facade client's request went into a dead
  endpoint (`netstackd: ipc recv err` once at facade start) — `SELFTEST: icmp ping FAIL`,
  `dsoftbus os connect FAIL`, and this package's egress probes, in headless as well as smp1
  (no log in `build/logs/` ever has `icmp ping ok`). Fix: init transfers the pair with
  `cap_transfer_to_slot` to 5/6 (deterministic, the dsoftbusd 3/4 pattern), so the facade is
  reachable again; the egress proofs run BEFORE the ICMP probe (its handler blocks the facade
  loop for its whole timeout) and wait on a 3 s time budget. ⤳ Follow-ups for the network
  lane: `SELFTEST: icmp ping ok` / `dsoftbus os connect ok` should now be re-evaluated (they
  were never ladder-gated in test-all).
- ⭐⭐ Finding 2 (fixed here, netstackd `main.rs` + `facade/runtime.rs`): after the slot fix the
  facade still served nobody — raw first-iteration tracing showed `poll` → `recv empty` → never a
  second iteration. netstackd demoted itself to `QosClass::Idle` after bring-up (commit e549a2e4,
  to keep the display/input path unstarved); the scheduler is strict-priority, so once any
  Normal task polled with `yield_()` (the selftest's RPC waits, dsoftbusd's connect retries) the
  Idle facade never ran again. Fix per RFC-0069's reactive-idle pattern: the facade stays at the
  Normal class and its recv is a TIMED kernel park (5 ms bound = smoltcp cadence) instead of
  NONBLOCK + yield — no CPU while idle, no starvation. Diagnostics removed after the fix.
  ⤳ The two findings together (spawn-time slot + Idle demotion) are why no recorded boot ever
  had `SELFTEST: icmp ping ok` / `dsoftbus os connect ok`.
- Proof: `cargo test -p security_v2_host` 5, OS-target strict check netstackd + selftest-client +
  init-lite, selftest arch gate; smp1 alone: `!cap-deny … dst=192.168.1.1:80` / `dst=10.0.2.2:8080`
  / `dst=192.168.1.2:443`, `SELFTEST: egress deny ok`, `egress allow ok`, `egress learn collected
  ok`, and the first recorded `SELFTEST: icmp ping ok` (2026-09-07 17:00); `just check` green,
  `just test-all` green 2026-09-07 (`exit=0`, all nine QEMU lanes, the three egress markers +
  `icmp ping ok` in every boot).
- Next: P4 (audit taxonomy `AuditReason::{QuotaExceeded, EgressDenied, IngressDenied}`, counters
  `quota_denies_total{subject}` / `egress_denies_total{subject}`, `docs/security/network-egress.md`,
  `sandboxing.md` boundary paragraph) — closes TASK-0043.

### Packages

- **P0 — Contract**: RFC-0072 amendment (`EDQUOTA` codes) + the schema v2 share of TASK-0028 P0
  (approval zones `docs/rfcs`, `source/libs`).
- **P1 — statefs quotas**: accounting in `userspace/statefs` (host-testable), enforcement in
  statefsd at the put seam, quota declaration in `policies/*.toml` (`[quota.<subject>]` soft/hard);
  host `tests/state_quota_host/` (from TASK-0133: deterministic accounting, deny-on-exceed with
  stable code, soft warn once per window); OS `SELFTEST: quota deny ok`.
- **P2 — netstackd identity + deny status** (shared with TASK-0052): `sid` on `FacadeContext`,
  `STATUS_DENY`, policyd authorize at connect/listen/bind, `test_reject_unattributed_connect`.
- **P3 — Egress policy**: profile evaluation at connect, host `tests/security_v2_host/`
  (`test_reject_egress_cidr`, `test_reject_egress_port`, allowed CIDR/port passes, learn mode
  records the attempt), OS `net-egress: enforced`, `SELFTEST: egress deny ok`, `SELFTEST: egress
  allow ok` (loopback + QEMU user-net targets, single-VM profile).
- **P4 — Audit taxonomy + counters + docs**: `AuditReason` variants, counters,
  `docs/security/network-egress.md` (new), `sandboxing.md` boundary paragraph (`device.mmio.net`
  grant = bypass), `abi-filters.md`, CHANGELOG, board.

### Touched paths (corrected)

`userspace/statefs/`, `source/services/statefsd/`, `userspace/vfs-types/` (VfsError, RFC-0072),
`source/services/netstackd/`, `userspace/policy/`, `source/services/policyd/`,
`source/libs/nexus-abi/` (via 0028 P0, approval), `policies/`, `tests/state_quota_host/`,
`tests/security_v2_host/`, `source/apps/selftest-client/`, `docs/security/`,
`scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` (approval, markers).

### Stop conditions (Definition of Done — replaces older DoD)

Host: `test_reject_write_over_hard_quota`, `test_reject_egress_cidr`, `test_reject_egress_port`,
`test_reject_unattributed_connect`, `test_reject_audit_reason_unknown` green; soft-warn-once
proven. OS: `statefs: quota deny`, `SELFTEST: quota deny ok`, `net-egress: enforced`,
`SELFTEST: egress deny ok`, `SELFTEST: egress allow ok` gated in headless/smp1; counters visible
via metricsd. Docs + CHANGELOG + board; TASK-0133 cross-referenced.


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
