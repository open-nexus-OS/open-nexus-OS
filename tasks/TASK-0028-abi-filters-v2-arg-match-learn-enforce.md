---
title: TASK-0028 ABI filters v2: argument matchers + learn→enforce + policy generator (host-first, OS-gated)
status: In Progress (P0 delivered 2026-09-05)
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0006
  - TASK-0008
  - TASK-0019
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Depends-on (ABI filter v1 dispatcher): tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md
  - Depends-on (audit/learn sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (policy authority): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Testing contract: scripts/qemu-test.sh
---

## End-state rewrite 2026-09-03 (binding; supersedes older sections where they differ)

Verified repo reality (Explore 2026-09-03): `source/libs/nexus-abi/src/abi_filter.rs` knows only
`SyscallClass {StatefsPut, NetBind}`, `AbiRule` has no size/deadline/address fields,
`encode_profile_v1` can express exactly two optional rules, the wire carries no epoch; `check_*`
are pure first-match-wins functions whose ONLY caller is the selftest — no service gates a real
operation on them today. Learn primitives exist capability-grain in `userspace/policy/src/lib.rs`
(`LearnObservation`, `learn_observations()`, `normalize_learn_log()`) and are never emitted.
policyd's OS-lite `AuditReason` has one variant. The profile schema is parsed twice
(`userspace/policy/src/lib.rs:36-48` host, `source/services/policyd/build.rs:23-42` OS build) —
every v2 field lands in BOTH. TASK-0229 (`policy.bin`) and TASK-0189 (per-process profiles) are
future consumers of the SAME profile. `userspace/security/` and `tools/abi-gen/` do not exist
(touched paths below are corrected). `docs/standards/SECURITY_STANDARDS.md:114` forbids runtime
policy modification — reconciled below.

### Goal (end state)

ONE policy-profile model (schema v2) that every enforcement point in the system evaluates the same
way: bounded argument matchers (`statefs` path prefix + payload size, `net.bind` port range +
address class, `net.connect` CIDR + port, `limits {deadline_ms, max_payload}`), longest-prefix-wins
with deny-beats-allow precedence, an epoch-guarded profile wire, a learn pipeline that records
would-deny observations to logd without ever bypassing a deny, and a generator that turns learn
logs into a conservative profile skeleton. The profile is served by policyd (single authority)
and enforced at the real seams (statefsd, netstackd facade) — not only asserted by the selftest.

### Non-goals

Kernel enforcement of raw `ecall`s (TASK-0188 — the kernel stays untouched); a second profile
tree for sandbox/process limits (TASK-0189 consumes this schema); a separate `abi-filterd` (policyd
is the authority, resolved 2026-08-14); full regular expressions (bounded literal sets only).

### Decisions

- **Schema v2** (`policies/*.toml`, `[abi_profile.<subject>]`): sub-tables `statefs`, `net.bind`,
  `net.connect`, `limits`, plus `epoch` (u32). Same sections become the input of TASK-0229's
  compiler and TASK-0189's `sys/ipc/vfs/limits` split — field names chosen once.
- **Precedence**: longest-prefix-wins, deny beats allow on equal length; documented behaviour
  change from first-match-wins, with `test_reject_first_match_shadowing` proving that a broad allow
  can no longer shadow a narrower deny.
- **Wire v2** (`encode_profile_v2`/`decode_profile_v2`, `MAX_PROFILE_BYTES` budget honoured,
  additive: v1 still decodes): rule records carry class, action, prefix/CIDR, port range, size and
  deadline bounds; header carries `epoch`. Stale epoch ⇒ deterministic reject.
- **Enforcement seams**: statefsd `policy_allows` (`os_lite.rs:547-591`) evaluates the subject's
  profile for `StatefsPut` after the capability check; netstackd facade evaluates `NetBind` /
  `NetConnect` (identity plumbing delivered by TASK-0043 P2). The selftest keeps its assertions but
  is no longer the only caller.
- **Learn pipeline**: profile evaluation in `Learn` mode emits `abi.learn` records to logd
  (scope `policyd.learn`, sampled + token-bucket bounded; deny-audit loop hazard from
  `logd/evidence.rs` respected). Generator = `nx policy learn-gen` (nx subcommand, no new tool
  directory) → conservative TOML skeleton (dedup, rule cap).
- **Mode switch**: OS-lite policyd op `OP_SET_ABI_MODE` (authenticated by `sender_service_id`,
  epoch-guarded, audited). Startup is always `Enforce`. `SECURITY_STANDARDS.md` gets the explicit
  exception „authenticated, epoch-guarded mode transitions through policyd are the ONLY runtime
  policy change“.
- **Deny reasons**: policyd `AuditReason` becomes the single deny taxonomy
  (`AbiRuleDenied{class}` here; quota/egress/ingress variants land with TASK-0043/0052).

### P0 delivered 2026-09-05 — RFC-0091 seed

- `docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md`: schema v2 (`epoch`,
  `limits`, `[[…statefs]]`, `[[…net.bind]]` with `address`, `[[…net.connect]]` with `cidr` +
  `ports`; v1 keys transcoded, mixed profiles rejected), precedence (longest specific match, deny
  beats allow, deny by default, limits after allow), wire v2 (20-byte header with epoch + flags,
  16-byte rule records + port ranges + prefix, `MAX_RULES` 24, `MAX_PROFILE_BYTES` 1024 — the
  policyd reply already carries `profile_len:u16le`; v1 keeps decoding), epoch (monotone per
  subject, `STATUS_STALE = 4`), learn record format + bounds, `OP_SET_ABI_MODE = 7` as the ONE
  runtime transition, seams, reject vocabulary, the `test_reject_*` names and the marker set. RFC
  index entry; ledgers 0043/0052 already reference „the TASK-0028 schema v2“.
- Recut vs. the plan text: `MAX_RULES` 16 → 24 and `MAX_PROFILE_BYTES` 512 → 1024 (24 rule
  records × 16 B + ports + prefixes do not fit 512); `STATUS_STALE` is a policyd status (additive
  `4`), not a new op; IPv6 CIDRs reserved (`af = 6` rejects) until netstackd has v6 sockets.
- Next: P1 (matcher + codec v2 + both parsers + reject suite).

### Packages

- **P0 — RFC seed „Policy profile v2: schema + wire“** (approval zones `docs/rfcs`,
  `source/libs/nexus-abi`, `source/libs/nexus-wire`): ONE approval covering `NetConnect`, the
  `NetBind` address dimension, size/deadline limits and the epoch — shared with TASK-0043/0052.
- **P1 — Matcher + codec v2 + reject suite**: `abi_filter.rs` (new classes/fields, precedence,
  v2 codec, epoch), both parsers, `source/libs/nexus-abi/tests/abi_filter_reject.rs` extended:
  `test_reject_regex_dos` (bounded literal set), `test_reject_argument_injection`,
  `test_reject_stale_profile_epoch`, `test_reject_unauthenticated_mode_switch`,
  `test_reject_first_match_shadowing`, `test_learn_roundtrip`. Command:
  `cargo test -p nexus-abi -- v2_reject --nocapture`.
- **P2 — Learn pipeline + generator**: emission in `userspace/policy` + policyd OS-lite (bounded),
  `nx policy learn-gen` with process-boundary tests (`tools/nx/tests/policy_cli.rs`).
- **P3 — OS enforcement + markers**: `OP_SET_ABI_MODE`, statefsd/netstackd evaluation, selftest
  drives learn → generate → enforce; markers `SELFTEST: abi learn collected ok`, `SELFTEST: abi
  enforce allow ok`, `SELFTEST: abi enforce deny ok`, `SELFTEST: abi mode switch auth ok`,
  `SELFTEST: abi stale epoch reject ok` gated in headless/smp1 (three-way marker SSOT). Docs:
  `docs/security/abi-filters.md` (lifecycle section), `docs/security/capabilities.md` (path
  `recipes/policy/` → `policies/`), `docs/standards/SECURITY_STANDARDS.md` exception.

### Touched paths (corrected)

`source/libs/nexus-abi/` (approval), `source/libs/nexus-wire/` (approval), `userspace/policy/`,
`source/services/policyd/`, `source/services/statefsd/` (evaluation call), `source/services/
netstackd/` (evaluation call, after 0043 P2), `tools/nx/` (`policy learn-gen`),
`source/apps/selftest-client/`, `policies/`, `docs/security/`, `docs/standards/`,
`scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` (approval, markers).

### Stop conditions (Definition of Done — replaces older DoD)

Host: the reject suite above green in both parsers; `nx policy learn-gen` produces a deterministic
skeleton from a fixture learn log. OS: the five `SELFTEST: abi …` markers in headless/smp1 with a
real deny observed at statefsd (and at netstackd once 0043 P2 landed). Docs + CHANGELOG + board.


## Rebase 2026-08-14 (heavy de-scope — most of this ledger already shipped)

Verified against the repo on 2026-08-14. **Do NOT re-implement** the following; it exists and is tested:

- **Argument-level matching shipped with v1 (TASK-0019, Done).** `source/libs/nexus-abi/src/abi_filter.rs`
  already implements statefs **path-prefix** matching (with payload-size bounds) and net-bind
  **port-range** matching: `matches_statefs_put` / `matches_net_bind` at
  `source/libs/nexus-abi/src/abi_filter.rs:189-207`, enforced via `check_statefs_put` (:248) and
  `check_net_bind` (:264), with negative tests in `source/libs/nexus-abi/tests/abi_filter_reject.rs`.
  The v2 plan items "path prefixes" and "port ranges" are therefore already green.
- **learn→enforce lifecycle + authenticated, epoch-guarded mode switching shipped via TASK-0047 (Done).**
  policyd already carries `PolicyMode::{Enforce, DryRun, Learn}` and an authenticated, stale-version-guarded
  `set_mode` (`source/services/policyd/src/std_server.rs:355-388`, `ensure_authorized` rejects
  unauthorized actors and stale observed versions fail-closed, with audit records). This ledger's two
  headline security tests are effectively green there:
  `test_reject_unauthenticated_mode_change` (`std_server.rs:969`) and
  `test_reject_stale_mode_change` (`std_server.rs:985`). policyd is the single profile/mode authority —
  the YELLOW "policyd vs abi-filterd" decision below is resolved: **policyd, no abi-filterd**.

**Honest residual scope** (what is actually left, if still wanted):

- Richer argument matchers **beyond** path-prefix/port-range: bounded regex allow-list, deadline
  (`deadline_ms`) bounds, `NetConnect { dst_port }` matching.
- The **learn-event pipeline**: rate-limited/sampled `abi.learn` emission to logd, plus the
  `abi-gen` generator tool (learn logs → conservative TOML skeleton).
- OS selftest markers for learn/enforce once the above exists.

Effective size shrinks **M → S**. Status stays Draft.

**Kernel untouched — unambiguous.** This task is and remains a pure userland guardrail
(`nexus-abi` + policyd); no kernel paths are in scope. (`tasks/STATUS-BOARD.md` wrongly lists
TASK-0028 as kernel-touching; that board entry is being fixed separately — this ledger is the truth.)

## Context

TASK-0019 defines ABI syscall guardrails in `nexus-abi` (userland, kernel untouched). v2 extends that
system with:

- argument-level matching (path prefixes/regex, port ranges, size/deadline),
- a **learn** mode to record real call traces,
- an **enforce** mode to deny calls that don’t match,
- a small generator tool to produce conservative TOML policies from learn logs.

Lifecycle handoff from `TASK-0019`:

- `TASK-0019` closes with boot-time/static profile lifecycle only.
- This task is the first allowed scope to add controlled runtime lifecycle transitions
  (`learn`/`enforce`, authenticated mode switching, bounded reload semantics).

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
- Lifecycle transitions must be authenticated and bounded:
  - mode switches require policy-authority authentication (`sender_service_id` bound),
  - reload paths require monotonic epoch/version checks,
  - stale or unauthenticated lifecycle transitions are rejected fail-closed.

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
- **YELLOW (lifecycle drift)**:
  - Keep TASK-0019 boundary intact: no implicit transition from static lifecycle to runtime lifecycle.
  - Startup behavior remains deterministic enforce/static unless an explicit authenticated transition occurs.

## Security considerations

### Threat model

- **Bypass via raw ecall**: Malicious code executes `ecall` directly, bypassing filters (same as TASK-0019)
- **Learn mode abuse**: Attacker uses learn mode to discover sensitive operations
- **Policy generator poisoning**: Attacker injects malicious patterns into learn logs
- **Regex DoS**: Attacker crafts patterns that cause exponential matching time
- **Argument injection**: Attacker crafts arguments to bypass prefix/range filters

### Security invariants (MUST hold)

- Learn mode MUST NOT bypass deny rules (learn-only, never grants access)
- Generated policies MUST be reviewed by humans before enforcement
- Regex patterns MUST be bounded and deterministic
- Argument matching MUST use longest-prefix-wins precedence
- Size/deadline bounds MUST be enforced before parsing
- Runtime mode changes MUST be authenticated and audit-visible (who changed mode, old->new, epoch)
- Stale profile epochs MUST be rejected deterministically

### DON'T DO

- DON'T use learn mode as a policy (it's for generating policies only)
- DON'T auto-apply generated policies without human review
- DON'T use unbounded regex patterns
- DON'T trust argument values from untrusted sources
- DON'T skip size bounds on learned operations
- DON'T allow unauthenticated runtime mode transitions
- DON'T accept stale profile/reload epochs

### Attack surface impact

- **NOT a security boundary**: Same as TASK-0019 - userland guardrail only
- **Learn mode risk**: Could expose sensitive operation patterns if logs leaked
- **Policy generation risk**: Automated policies may be too permissive

### Mitigations

- Learn mode logs are rate-limited and sampled (not exhaustive)
- Generated policies are conservative (tighten prefixes, cap sizes)
- Regex engine is deterministic with bounded backtracking
- Longest-prefix-wins precedence prevents bypass via path traversal
- All generated policies require human review before enforcement
- Runtime transitions are authenticated + epoch-guarded with deterministic reject behavior

### Security proof

#### Audit tests (negative cases)

- Command(s):
  - `cargo test -p nexus-abi -- v2_reject --nocapture`
- Required tests:
  - `test_reject_regex_dos` — exponential pattern → rejected
  - `test_reject_argument_injection` — path traversal → denied
  - `test_learn_roundtrip` — learn→generate→enforce deterministic
  - `test_reject_unauthenticated_mode_switch` — unauthorized lifecycle transition rejected
  - `test_reject_stale_profile_epoch` — stale epoch rejected deterministically

#### Hardening markers (QEMU)

- `SELFTEST: abi learn collected ok` — learn mode works
- `SELFTEST: abi enforce allow ok` — enforce mode allows matching ops
- `SELFTEST: abi enforce deny ok` — enforce mode denies mismatches

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
- lifecycle transitions:
  - authenticated mode switch works and is audited with stable fields
  - unauthenticated/stale-epoch transitions are rejected fail-closed

### Proof (OS / QEMU) — after TASK-0019 + TASK-0006

Extend `scripts/qemu-test.sh` (order tolerant):

- `abi-profile: ready (server=policyd|abi-filterd)`
- `SELFTEST: abi learn collected ok`
- `SELFTEST: abi enforce allow ok`
- `SELFTEST: abi enforce deny ok`
- `SELFTEST: abi mode switch auth ok`
- `SELFTEST: abi stale epoch reject ok`

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
- `docs/testing/README.md`
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
