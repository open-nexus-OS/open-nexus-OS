# ABI syscall guardrails (TASK-0019 → TASK-0028, RFC-0091)

## Scope and boundary

ABI syscall filters are a **userspace guardrail** for compliant binaries,
evaluated by the service that owns the governed operation (statefsd for
`statefs.put`, netstackd for `net.bind` / `net.connect`). They are **not** a
hard sandbox against malicious code issuing raw `ecall` instructions.

- Kernel remains unchanged.
- True raw-ecall enforcement is deferred to kernel scope (`TASK-0188`).
- Contract: `docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md`.

## Profile schema v2 (`policies/*.toml`)

One grammar, one parser: `userspace/policy/src/schema.rs` is compiled by the
host `policy` crate and included by path in policyd's `build.rs`, so the OS
table and the host tooling can never accept different rules. The shared
fixture corpus lives in `policies/tests/` (`ok_*` / `reject_*`, run by
`userspace/policy/tests/schema_corpus.rs`).

```toml
[abi_profile."selftest-client"]
epoch = 1                      # monotone per subject; bump on every behavioural change

[abi_profile."selftest-client".limits]
deadline_ms = 2000             # optional request-deadline ceiling
max_payload = 4096             # optional statefs.put payload ceiling (default 4096)

[[abi_profile."selftest-client".statefs]]
action = "allow"               # allow | deny
prefix = "/state/app/selftest/" # bounded literal (≤ 64 bytes, absolute, canonical)
max_payload = 2048             # optional per-rule ceiling (≤ limits.max_payload)

[[abi_profile."selftest-client".net.bind]]
action = "allow"
ports = ["1024-65535"]         # ≤ 4 ranges, "n" or "a-b", 1..=65535
address = "loopback"           # loopback (default) | any

[[abi_profile."selftest-client".net.connect]]
action = "allow"
cidr = "10.0.2.0/24"           # IPv4, canonical network bits
ports = ["53", "80", "443"]
```

- Legacy v1 keys (`statefs_put_allow_prefix`, `net_bind_min_port`) still
  parse and transcode to `statefs allow` + loopback-only `net.bind allow`
  with `epoch = 0`. Mixing v1 keys and v2 sections is a parse error.
- Prefixes are literals: any pattern byte (`* ? [ ] ( ) { } | ^ $ + \`),
  control byte, `..`/`.`/empty segment or relative path is rejected at parse
  time (`test_reject_regex_dos`); the matcher never sees a pattern.
- Unknown keys or sections are a parse error (fail closed), never ignored.
- Bounds: 24 rules per subject, 4 port ranges per rule, 1024 wire bytes.

## Precedence (RFC-0091 §2)

For one (subject, class, argument tuple): collect every accepting rule, pick
the most specific (`statefs` — longest prefix; `net.bind` — narrowest port
range, then `loopback` over `any`; `net.connect` — longest CIDR, then
narrowest range), **deny beats allow** on equal specificity, no accepting rule
is `Deny`. An `Allow` is then checked against `limits` (`max_payload`,
`deadline_ms`). This replaced v1's first-match-wins: a broad `allow` can no
longer shadow a narrower `deny` regardless of rule order
(`test_reject_first_match_shadowing`). Matching is O(rules × bytes) and
argument-canonical: a `statefs` path with `..`, `.`, empty segments, NUL or
over 128 bytes is denied before any rule is consulted
(`test_reject_argument_injection`).

## Wire v2

`source/libs/nexus-abi/src/abi_filter/wire.rs`: 20-byte header (`'A','F'`,
version 2, rule count, subject, epoch, flags), optional 8-byte limits record,
16-byte rule records + port ranges + prefix bytes. Unknown class, action,
address class, flag or reserved byte, trailing bytes and oversize all reject
(`test_reject_unknown_class_fails_closed`, `test_reject_oversized_profile_v2`).
v1 frames keep decoding (epoch 0). policyd serves v2 on `OP_ABI_PROFILE_GET`
(`source/services/policyd/src/abi_profile.rs`); an un-profiled subject gets
the empty deny-all profile.

## Security invariants

- Profile distribution is accepted only from the authenticated policy
  authority identity (`sender_service_id` must match `policyd`).
- Profile subject binding is kernel-derived (`service_id`), never payload text.
- Decode/matching is bounded (profile bytes, rule count, prefix/path sizes,
  port ranges, matcher cost); decisions are deny-by-default and deterministic.
- Epoch is monotone per subject: a consumer holding epoch N treats a profile
  with epoch < N as stale and keeps its cached one
  (`test_reject_stale_profile_epoch`).

## Evaluation and learn pipeline (RFC-0091 §5–§7)

- **Where**: policyd evaluates. A seam (statefsd `put`, netstackd
  bind/connect) sends `OP_ABI_EVAL` with the subject it serves (a privileged,
  init-wired proxy; any other sender may only evaluate itself,
  `test_reject_eval_subject_spoof`) and the argument tuple; policyd applies
  the profile, the `limits` and the mode in one place
  (`source/services/policyd/src/abi_eval.rs`).
- **Learn mode** (`source/services/policyd/src/abi_learn.rs`): decisions are
  unchanged; a refused evaluation additionally emits ONE record to logd scope
  `policyd.learn`: `abi.learn epoch=<u32> subject=<sid hex16>
  class=<statefs|net.bind|net.connect> arg=<prefix|port/loopback|port/any|a.b.c.d:port>
  would=<deny|limit>` (≤ 160 bytes; the statefs argument is the path's
  directory prefix, ≤ 64 bytes). Bounds: dedup ring of 64 (subject, class,
  arg) keys, token bucket 8/s burst 32, `abi.learn.dropped` counter; a
  failed delivery is counted, never raised to the seam.
- **One format**: `userspace/policy/src/learn_record.rs` is compiled by the
  host `policy` crate and included by path in policyd, so the writer and
  the reader cannot drift (it also holds `service_id_from_name`, the one
  FNV every policy tool uses).
- **Generator**: `nx policy learn-gen <learn-log> --subject <name> --out <toml>
  [--allow-any]` turns a logd/UART dump into a review-first
  `[abi_profile.<name>]` skeleton (`# generated — review before enabling`):
  dedup, sorted, capped at 24 rules (the cap is reported, never silent),
  `epoch` = observed + 1, `allow` rules for every refused argument; an
  any-interface bind is emitted commented-out unless `--allow-any`, a
  connect rule is always the observed /32. The output compiles through the
  shared schema (`test_learn_roundtrip`: learn → generate → parse → the
  matcher allows exactly the observed arguments).

## Lifecycle

- Profiles are static per boot: fetched/applied at startup, never hot-reloaded.
- The ONE runtime transition is the authenticated, epoch-guarded per-subject
  mode switch `OP_SET_ABI_MODE` (Enforce ↔ Learn; RFC-0091 §6, TASK-0028 P3).
  Every boot starts in Enforce; the mode table is never persisted.

## Marker contract

- `abi-profile: ready (server=policyd|abi-filterd)`
- `abi-filter: deny (subject=<svc> syscall=<op>)`
- `SELFTEST: abi filter deny ok`
- `SELFTEST: abi filter allow ok`
- `SELFTEST: abi netbind deny ok`
- TASK-0028 P3 adds the enforcement/learn/mode-switch markers listed in RFC-0091.

## Required negative host proofs

```bash
cargo test -p nexus-abi -- v2_reject --nocapture   # RFC-0091 suite
cargo test -p nexus-abi --test abi_filter_reject   # v1 suite (still green on v2 precedence)
cargo test -p policy --test schema_corpus          # shared grammar corpus
cargo test -p policyd                              # served profile = authored semantics
```

- v1 (TASK-0019): `test_reject_unbounded_profile`,
  `test_reject_unauthenticated_profile_distribution`,
  `test_reject_subject_spoofed_profile_identity`,
  `test_reject_profile_rule_count_overflow`,
  `test_reject_first_match_precedence_conflict_is_deterministic`,
  `test_reject_trailing_profile_bytes_as_malformed`,
  `test_reject_statefs_put_oversized_payload_fail_closed`,
  `test_reject_typed_distribution_subject_mismatch`.
- v2 (TASK-0028 P1): `test_reject_first_match_shadowing`,
  `test_reject_argument_injection`, `test_reject_regex_dos` (matcher and
  parser), `test_reject_stale_profile_epoch`,
  `test_reject_unknown_class_fails_closed`, `test_reject_oversized_profile_v2`.
- policyd frame handling: `test_abi_profile_get_v2_malformed_frame_is_fail_closed`,
  `test_abi_profile_get_v2_allows_privileged_proxy_subject_mismatch`,
  `selftest_profile_is_v2_with_authored_semantics`.
- learn pipeline (TASK-0028 P2): `test_learn_roundtrip` (policyd),
  `test_reject_eval_subject_spoof`, `learn_emission_is_rate_limited_and_counts_drops`,
  `cargo test -p nx --test policy_cli` (process boundary), `cargo test -p policy`
  (record format, generator).
- Pending with its package: `test_reject_unauthenticated_mode_switch` (P3).

## Anti-fake-green note

- Selftest marker emission is behavior-coupled: ABI `ok` markers are emitted
  only after profile subject binding matches the local kernel-derived
  `selftest-client` service identity and policy decisions are verified.
