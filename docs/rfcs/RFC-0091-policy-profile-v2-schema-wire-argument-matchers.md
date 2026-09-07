# RFC-0091: Policy profile v2 — schema, argument matchers, precedence, epoch-guarded wire

- Status: Draft (contract seed; execution TASK-0028, consumers TASK-0043 / TASK-0052 / TASK-0189 / TASK-0229)
- Owners: @runtime @security
- Created: 2026-09-05
- Last Updated: 2026-09-05
- Links:
  - Tasks: `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md` (execution + proof),
    `tasks/TASK-0043-security-v2-sandbox-quotas-egress-abi-audit.md` (egress = `net.connect`),
    `tasks/TASK-0052-security-v3-ingress-policy-ingressd-gateway.md` (ingress = `net.bind` address class)
  - Related RFCs: RFC-0019 (ABI guardrails v1 — this RFC supersedes its profile wire),
    RFC-0072 (VFS v2 — quota codes land there, not here), RFC-0068 (verdict/markers)
  - Standards: `docs/standards/SECURITY_STANDARDS.md` (the ONE runtime-change exception is §6 below),
    `docs/security/abi-filters.md` (operational doc, updated by TASK-0028 P3)

## Status at a Glance

- **Phase 0 (contract seed — this document)**: ✅ 2026-09-05 (TASK-0028 P0)
- **Phase 1 (matcher + codec v2 + reject suite)**: ⬜ TASK-0028 P1
- **Phase 2 (learn pipeline + `nx policy learn-gen`)**: ⬜ TASK-0028 P2
- **Phase 3 (OS enforcement seams + mode switch + markers)**: ⬜ TASK-0028 P3 (statefsd), TASK-0043 P2/P3 (netstackd identity + egress), TASK-0052 P1 (ingress address class)

Definition: “Complete” means the contract below is implemented and its proof gates (host reject suite + QEMU markers) are green.

## Scope boundaries (anti-drift)

This RFC is a **contract seed**. Implementation planning and proofs live in the tasks.

- **This RFC owns**:
  - the policy-profile **schema v2** (`policies/*.toml`, `[abi_profile.<subject>]`) — ONE schema for ABI argument filters, egress, ingress and (later) process limits;
  - the **precedence model** (longest-specific-match wins, deny beats allow, deny by default);
  - the **profile wire v2** (`encode_profile_v2` / `decode_profile_v2`, header epoch, bounded rule records) and its bounds;
  - the **epoch** contract (monotone per subject, stale ⇒ deterministic reject);
  - the **learn** record format and its bounds;
  - the **mode switch** op (`OP_SET_ABI_MODE`) and the ONLY sanctioned runtime policy transition;
  - the **deny taxonomy** entry `AbiRuleDenied{class}` in policyd's `AuditReason`;
  - the reject vocabulary and the required `test_reject_*` proofs.
- **This RFC does NOT own**:
  - kernel enforcement of raw `ecall`s (TASK-0188 — the kernel stays untouched);
  - quotas / `EDQUOTA` wire codes (RFC-0072 amendment, TASK-0043 P0);
  - the `ingress` policy DOMAIN and `ingressd` (RFC seed by TASK-0052 P0; it consumes the `net.bind` address class defined here);
  - `policy.bin` compilation (TASK-0229) and the per-process `sys/ipc/vfs/limits` split (TASK-0189) — both consume this schema, they do not redefine it;
  - regular expressions of any kind (bounded literal matchers only — §Constraints).

### Relationship to tasks (single execution truth)

TASK-0028 P1–P3 implement and prove Phases 1–3 of this RFC for the `statefs` seam and the mode/learn machinery; TASK-0043 P2/P3 and TASK-0052 P1 implement the netstackd seams over the SAME schema and codec. Stop conditions and proof commands live in those ledgers.

## Context

Verified repo reality (2026-09-03): `source/libs/nexus-abi/src/abi_filter.rs` knows two classes
(`StatefsPut`, `NetBind`), rules carry only a path prefix or a port range, `encode_profile_v1` can
express exactly two optional rules, the wire carries no epoch, matching is first-match-wins, and the
ONLY caller of the `check_*` functions is the selftest — no service gates a real operation on a
profile. The TOML schema (`statefs_put_allow_prefix`, `net_bind_min_port`) is parsed twice
(`userspace/policy/src/lib.rs` for the host tooling, `source/services/policyd/build.rs` for the OS
build). Egress (TASK-0043), ingress (TASK-0052), process profiles (TASK-0189) and the compiled
`policy.bin` (TASK-0229) all need argument-grain rules — without ONE schema they would fork four.

## Goals

- ONE profile model evaluated identically by every enforcement point (policyd serves it; statefsd and the netstackd facade evaluate it; the selftest asserts it).
- Bounded argument matchers: statefs path prefix + payload size, `net.bind` port range + address class, `net.connect` CIDR + port set, per-subject limits.
- A precedence model that cannot be shadowed: longest specific match, deny beats allow, deny by default.
- An additive wire v2 with an epoch so a stale profile or a stale mode request is a deterministic reject, never a silent downgrade.
- A learn pipeline that records would-deny observations WITHOUT ever bypassing a deny, and a generator that turns them into a conservative profile skeleton.
- Exactly one sanctioned runtime policy transition (the authenticated, epoch-guarded mode switch), reconciled with `SECURITY_STANDARDS.md`.

## Non-Goals

- Kernel-side syscall filtering (TASK-0188).
- Regex/glob engines; unbounded rule sets; per-packet DPI.
- A second authority (`abi-filterd`) — policyd IS the authority (decided 2026-08-14).
- IPv6 CIDRs in v2 (reserved: `af = 6` is a decode reject until an amendment lifts it).

## Constraints / invariants (hard requirements)

- **Determinism**: the decision for (subject, class, arguments, profile) is a pure function; the same inputs yield the same `RuleAction` on host and OS.
- **Deny by default**: no matching rule ⇒ `Deny`. An empty profile denies every governed class. A subject without a profile denies every governed class.
- **No fake success**: `SELFTEST: abi … ok` markers only after the real seam (statefsd / netstackd) returned the decision; `learn` records only when a real evaluation happened.
- **Bounded resources** (normative numbers, re-sourced by code from ONE place — `nexus_abi::abi_filter`):
  - `MAX_RULES = 24` per profile, `MAX_PROFILE_BYTES = 1024` (raised from 512; the policyd reply carries `profile_len:u16le` so the wire needs no change),
  - `MAX_PATH_PREFIX_BYTES = 64`, `MAX_STATEFS_PATH_BYTES = 128`, `MAX_STATEFS_PUT_BYTES = 4096`,
  - `MAX_PORT_RANGES_PER_RULE = 4`, CIDR prefix length 0..=32 (IPv4 only in v2),
  - `MAX_LEARN_ENTRIES = 64` per subject (dedup set), learn rate ≤ 8 records/s per subject, burst 32, sampled 1:1 below the bucket, dropped (counted) above it,
  - matcher cost O(rules × prefix bytes) with no allocation.
- **Identity**: the subject is `sender_service_id` from kernel IPC — never a payload string. Profile distribution is accepted only from policyd's kernel-attributed id.
- **Security floor**: bring-up boots run `Enforce`; `Learn` is a per-boot state that CANNOT survive a reboot and CANNOT be entered without an authenticated, epoch-guarded request.
- **Stubs policy**: none. Until a seam evaluates the profile it prints nothing; there is no `abi enforce … ok` marker for a seam that does not evaluate.

## Proposed design

### 1. Schema v2 (normative, `policies/*.toml`)

```toml
[abi_profile."selftest-client"]
epoch = 3                      # u32, monotone per subject (see §4); absent ⇒ v1 shape (epoch 0)

[abi_profile."selftest-client".limits]
deadline_ms = 2000             # optional; per-request deadline ceiling the seam enforces
max_payload = 4096             # optional; overrides MAX_STATEFS_PUT_BYTES downward only

[[abi_profile."selftest-client".statefs]]
action = "allow"
prefix = "/state/app/selftest/"
max_payload = 4096             # optional per-rule ceiling (≤ limits.max_payload)

[[abi_profile."selftest-client".statefs]]
action = "deny"
prefix = "/state/app/selftest/secrets/"   # narrower deny wins over the broader allow (§2)

[[abi_profile."selftest-client".net.bind]]
action = "allow"
ports = ["1024-65535"]         # ≤ MAX_PORT_RANGES_PER_RULE ranges, "n" or "a-b", inclusive
address = "loopback"           # "loopback" (default) | "any" — TASK-0052: "any" needs an exposure intent

[[abi_profile."selftest-client".net.connect]]
action = "allow"
cidr = "10.0.2.0/24"           # IPv4 CIDR; "0.0.0.0/0" = everywhere
ports = ["80", "443", "1024-2048"]
```

- v1 keys (`statefs_put_allow_prefix`, `net_bind_min_port`) remain accepted by BOTH parsers and are transcoded to v2 rules (`statefs allow prefix`, `net.bind allow ports = ["min-65535"] address = "loopback"`) with `epoch = 0`. A profile that mixes v1 keys and v2 sections is a parse error (no half-migrated profiles).
- Both parsers (`userspace/policy` host, `policyd/build.rs` OS build) MUST accept the identical grammar; TASK-0028 P1 adds a shared test corpus under `policies/tests/` that both run.
- Class vocabulary v2: `statefs` (`statefs.put`), `net.bind` (bind/listen/udp-bind), `net.connect`. Unknown sections are a parse error (fail closed), not ignored.

### 2. Precedence (normative)

For one (subject, class, argument tuple):

1. Collect every rule of that class whose matcher accepts the arguments.
2. Pick the most specific: `statefs` — longest `prefix`; `net.bind` — narrowest port range (fewest ports) then `address` `loopback` over `any`; `net.connect` — longest CIDR prefix length, then narrowest port range.
3. On equal specificity, **deny beats allow**.
4. No accepted rule ⇒ `Deny`.
5. An `Allow` is then checked against `limits` (`max_payload`, `deadline_ms`); exceeding a limit is `Deny` with reason `limit`.

This replaces v1's first-match-wins. Behaviour change: a broad `allow` can no longer shadow a narrower `deny` regardless of rule order (`test_reject_first_match_shadowing`).

### 3. Wire v2 (normative, additive)

Header (20 bytes): `magic 'A','F'`, `version = 2`, `rule_count:u8`, `subject_id:u64le`, `epoch:u32le`, `flags:u32le` (bit 0 = `has_limits`; other bits reserved = 0 ⇒ decode reject). Optional limits record (8 bytes: `max_payload:u32le`, `deadline_ms:u32le`) when `has_limits`. Then `rule_count` rule records:

```
class:u8        1 = statefs, 2 = net.bind, 3 = net.connect   (unknown ⇒ MalformedProfile)
action:u8       0 = deny, 1 = allow
prefix_len:u8   0..=64 (statefs); 0 for net classes
addr_class:u8   0 = loopback, 1 = any (net.bind); 0 for others
port_count:u8   0..=4 port ranges follow the fixed part
cidr_len:u8     0..=32 (net.connect); 0 for others
reserved:u16    = 0
cidr:[u8;4]     IPv4 address (net.connect), zero otherwise
max_payload:u32le   0 = inherit
port ranges:    port_count × (min:u16le, max:u16le)
prefix bytes:   prefix_len bytes (statefs)
```

- `decode_profile_v1` keeps decoding version 1 (until every producer emits v2; v1 profiles decode with `epoch = 0` and v2 precedence).
- Total encoded size ≤ `MAX_PROFILE_BYTES`; oversize ⇒ `OversizedProfile` (never truncation).
- policyd's `OP_ABI_PROFILE_GET` reply is unchanged in shape (`profile_len:u16le` + bytes); the epoch travels inside the profile header. A consumer that cached epoch N and receives N' < N treats the profile as stale and keeps its cached one (`test_reject_stale_profile_epoch` at the consumer).

### 4. Epoch (normative)

- `epoch` is per subject, monotone, authored in the policy TOML; the compiled policy set carries it; policyd serves it in every profile.
- Every profile-dependent request that can change behaviour (`OP_SET_ABI_MODE`) names the epoch it was authored against; policyd rejects a request whose epoch ≠ the subject's current epoch with `STATUS_STALE` (new policyd status `4`, additive).
- Learn records carry the epoch, so a generated skeleton is bound to the profile it was observed under.

### 5. Learn pipeline (normative)

- Mode per subject: `Enforce` (default, every boot) or `Learn`. In `Learn`, the decision returned to the seam is UNCHANGED (a deny still denies); additionally the evaluation emits a learn record.
- Record (logd scope `policyd.learn`, one line, ≤ 160 bytes): `abi.learn epoch=<u32> subject=<sid hex> class=<statefs|net.bind|net.connect> arg=<prefix|port[/addr]|cidr:port> would=<deny|limit>`.
- Bounds: per-subject dedup set of `MAX_LEARN_ENTRIES` (subject, class, arg) keys — a repeated key emits nothing; token bucket 8/s, burst 32; drops are counted (`abi.learn.dropped` counter) and NEVER block the seam. The deny-audit loop hazard (`logd/evidence.rs`) applies: learn records are best-effort, non-persisted evidence class.
- Generator `nx policy learn-gen <learn-log> --subject <name> --out <toml>`: dedup, sort, cap at `MAX_RULES`, emit `allow` rules for observed `would=deny` arguments as a SKELETON marked `# generated — review before enabling`; never emits `address = "any"` or `0.0.0.0/0` without an explicit `--allow-any` flag.

### 6. Mode switch — the ONE runtime policy transition (normative)

- policyd OS-lite op `OP_SET_ABI_MODE = 7`: request `{nonce:u32le, subject_id:u64le, mode:u8 (0 Enforce, 1 Learn), epoch:u32le}`; reply `{nonce, status}` with `STATUS_ALLOW | STATUS_DENY | STATUS_STALE | STATUS_UNSUPPORTED`.
- Authentication (amendment 2026-09-07, TASK-0028 P3): the request's `sender_service_id` (kernel-attributed) must hold the `policy.abi_mode` capability in the compiled policy (deny-by-default; proof boots grant it to `selftest-client`, production grants it to the `nx policy` device channel once TASK-0229 lands) — an allowlist in the policy SSOT, never a name in code; a privileged proxy does NOT bypass it. Any other sender ⇒ `STATUS_DENY` + audit (`test_reject_unauthenticated_mode_switch`, host: policyd `abi_mode.rs`).
- Epoch-guarded (§4), audited (`policyd: abi mode subject=<sid> mode=<m> epoch=<e>` + logd audit record), never persisted (a reboot returns to `Enforce`).
- `docs/standards/SECURITY_STANDARDS.md` §4 gets the explicit exception: „authenticated, epoch-guarded mode transitions through policyd are the ONLY runtime policy change; profiles themselves never change at runtime“.

### 7. Enforcement seams (contract-level)

- Governed subjects (amendment 2026-09-07, TASK-0028 P3): a subject is governed by the argument filters when a profile is authored for it; `OP_ABI_EVAL` for an un-profiled subject answers `STATUS_UNSUPPORTED` and the seam applies capability checks only. Authoring a profile for every statefs writer and then flipping un-profiled to deny is the tracked follow-up (TASK-0028 ledger) — flipping first would brick boot on the first unauthored prefix. Consumers of `OP_ABI_PROFILE_GET` still receive the explicit deny-all profile for un-profiled subjects.
- The seam holds `policy.delegate` (statefsd, netstackd — the same trust as the delegated capability check): it may name the subject it serves in `OP_ABI_EVAL`; any other sender may only evaluate itself.
- Evaluation lives in policyd (amendment 2026-09-05, TASK-0028 P2): a seam sends `OP_ABI_EVAL = 8` (`{nonce:u32le, subject_id:u64le, class:u8, addr_class:u8, port:u16le, addr_be:u32le, payload_len:u32le, deadline_ms:u32le, path:bytes8(≤128)}`, reply `STATUS_ALLOW|DENY|MALFORMED|UNSUPPORTED`) and policyd — which already holds the profile table, the mode table and the learn collector — decides, checks `limits` and emits the learn record in one place. A privileged proxy (an init-wired seam) names the subject it serves; any other sender may only evaluate itself. `OP_ABI_PROFILE_GET` stays for consumers that hold a profile (the selftest's assertions, epoch caching).
- statefsd `put`: after the capability check, evaluate `statefs` for (subject, path, payload_len) and `limits` via `OP_ABI_EVAL`; deny ⇒ `STATUS_DENIED` to the caller + `AuditReason::AbiRuleDenied{class: statefs}`.
- netstackd facade (identity plumbing by TASK-0043 P2): `net.bind` at bind/listen/udp-bind with (port, address class); `net.connect` at connect with (addr, port). Deny ⇒ `STATUS_DENY` + `AuditReason::AbiRuleDenied{class}` (TASK-0043 adds `EgressDenied`, TASK-0052 `IngressDenied` as the user-facing reasons layered on the same evaluation).
- The selftest remains a caller (assertions), never the only one.

### Phases / milestones (contract-level)

- **Phase 0**: this seed (schema, precedence, wire, epoch, learn, mode switch, seams).
- **Phase 1**: matcher + codec v2 + both parsers + host reject suite (TASK-0028 P1).
- **Phase 2**: learn emission + generator (TASK-0028 P2).
- **Phase 3**: OS seams + `OP_SET_ABI_MODE` + markers (TASK-0028 P3, TASK-0043 P2/P3, TASK-0052 P1).

## Security considerations

- **Threat model**: confused deputy (a service asking on behalf of another) — closed by kernel-attributed identity; argument injection (a path/port/CIDR crafted to match an allow while meaning something else) — closed by canonicalization before matching (statefs paths are canonicalized by statefsd, ports/addresses are numeric) and bounded literal matchers (`test_reject_argument_injection`); matcher DoS (pathological patterns) — closed by literal-only matchers with O(rules × bytes) cost (`test_reject_regex_dos` proves the parser rejects any pattern syntax); stale-profile downgrade — closed by the monotone epoch; runtime tampering — closed by the single audited transition.
- **Mitigations**: deny by default; deny beats allow; bounded everything; learn never bypasses deny; the mode switch is authenticated + epoch-guarded + audited + non-persistent.
- **Open risks**: the netstackd seams are only as strong as the sender plumbing (TASK-0043 P2); until then egress/ingress rules are contract, not enforcement — the tasks say so in their markers, this RFC does not claim it.

## Failure model (normative)

- Malformed / oversize / unknown-class / unknown-flag profile ⇒ decode reject (`MalformedProfile` / `OversizedProfile`); the consumer keeps its last good profile (or the empty deny-all profile) — never a partially applied one.
- Stale epoch (mode switch, cached profile) ⇒ `STATUS_STALE` / keep cached; deterministic, audited.
- Learn emission failure (logd down, bucket empty) ⇒ counted drop, decision unaffected.
- No silent fallback anywhere; every reject has a stable label from the vocabulary: `malformed | oversize | unknown-class | stale-epoch | unauthenticated | limit | deny`.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-abi -- v2_reject --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p policy && cargo test -p policyd
cd /home/jenning/open-nexus-OS && cargo test -p nx --test policy_cli
```

Required negative tests (names are the contract): `test_reject_regex_dos`, `test_reject_argument_injection`, `test_reject_stale_profile_epoch`, `test_reject_unauthenticated_mode_switch`, `test_reject_first_match_shadowing`, `test_reject_unknown_class_fails_closed`, `test_reject_oversized_profile_v2`, plus `test_learn_roundtrip` (learn log → `learn-gen` → parse → evaluate = the observed argument is now allowed, nothing else is).

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just test-os headless   # and smp1
```

### Deterministic markers

- `SELFTEST: abi learn collected ok` — a real would-deny evaluation was admitted by policyd's learn collector (`OP_ABI_LEARN_STATS = 9`, authority-gated: `admitted` +1 for the Learn-mode refusal, +0 for the identical Enforce-mode one). Delivery to logd is best-effort (§5) and witnessed separately by `SELFTEST: abi learn delivered ok|dropped` (not ladder-gated: the icount profile saturates policyd's logd path — the same `policyd: audit emit deferred` baseline).
- `SELFTEST: abi enforce allow ok` / `SELFTEST: abi enforce deny ok` — statefsd's `put` returned the profile's decision for a governed path.
- `SELFTEST: abi mode switch auth ok` — the authenticated, epoch-bound switches (Learn, then back to Enforce) applied and were audited (`policyd: abi mode …`). A proof boot has ONE authority sender, so the unauthenticated denial is the host reject test (`test_reject_unauthenticated_mode_switch`), not a marker.
- `SELFTEST: abi stale epoch reject ok` — a switch against an old epoch was rejected `STATUS_STALE`.
- `policyd: abi mode subject=<sid> mode=<m> epoch=<e>` — the audited transition.
- Network seams (TASK-0043/0052): `SELFTEST: egress deny ok`, `SELFTEST: ingress deny ok` — defined in those ledgers over this schema.

## Alternatives considered

- Keep first-match-wins with authored ordering — rejected: rule order is not a security property a reviewer can see; shadowing bugs are silent.
- Regex/glob matchers — rejected: unbounded cost and injection surface; literal prefixes + numeric ranges cover every current seam.
- A separate egress/ingress schema — rejected: four consumers would fork the model; one schema keeps policyd the single authority.
- Persisting `Learn` mode across boots — rejected: a device must never boot into a permissive state.

## Open questions

- IPv6 CIDRs (`af = 6`) — reserved; lifted by amendment when netstackd has v6 sockets (owner @runtime).
- Whether TASK-0189's `limits` split reuses this `limits` table verbatim or adds `ipc`/`vfs` sections — decided in TASK-0189 P0 (this RFC keeps `limits` open-ended: unknown keys are a parse error until then).

## Implementation Checklist

- [x] **Phase 0**: contract seed (this RFC), RFC index, ledgers 0028/0043/0052 point here — proof: `just check` docs gates (2026-09-05)
- [x] **Phase 1**: matcher + codec v2 + ONE shared parser (`userspace/policy/src/schema.rs`, included by policyd build.rs) + corpus `policies/tests/` + reject suite — proof: `cargo test -p nexus-abi -- v2_reject` 9/9 (2026-09-05)
- [x] **Phase 2**: learn (policyd `OP_ABI_EVAL` + bounded collector, logd scope `policyd.learn`) + `nx policy learn-gen` — proof: `cargo test -p nx --test policy_cli`, `cargo test -p policyd test_learn_roundtrip` (2026-09-05)
- [x] **Phase 3**: statefsd seam (`OP_ABI_EVAL`), `OP_SET_ABI_MODE` (cap-authenticated, epoch-guarded, audited), audit reasons, five markers in headless/smp1 (2026-09-07); netstackd seams follow TASK-0043 P2 / TASK-0052 P1
- [x] Task(s) linked with stop conditions + proof commands (TASK-0028, TASK-0043, TASK-0052).
- [x] QEMU markers appear in `scripts/qemu-test.sh` + proof-manifest and pass (statefs seam; network seam markers with TASK-0043/0052).
- [x] Security-relevant negative tests exist (`test_reject_*` above).
