---
title: TASK-0053 Security v3 (Recovery): signed Recovery Action tokens (.nxra) + replay protection — enforced on the recovery ops surface
status: Done
owner: @security
created: 2025-12-23
updated: 2026-08-24
completed: 2026-08-24
depends-on:
  - TASK-0008B # keystored / device keys (Done — satisfied)
  - TASK-0009 # statefs / /state (Done — satisfied)
  - TASK-0047 # Policy-as-Code (Done — satisfied)
  - TASK-0051 # recovery ops surface (enforcement point)
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md
  - Ops surface (enforcement point): tasks/TASK-0051-recovery-operations-surface.md
  - Boot targets: tasks/TASK-0050-system-reset-boot-targets-bootctld.md
  - Policy as Code (trust + gating): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Keystore / device keys: tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## Rewrite 2026-08-18 (enforcement point changed; format work unchanged)

The 2026-08-14 metadata correction stands (deps satisfied; `userspace/nxra/`;
`policies/` + `schemas/policy.config.schema.json`; **RFC seed required** for the
`.nxra` format before building). What changes with the lane rewrite:

- **Enforcement moves from `recovery-sh` to the TASK-0051 ops surface.** There
  is no shell (TASK-0050B Deferred). Mutating recovery ops (`fsck repair`,
  `slot switch`, `target set`, future flash/reset verbs) carry a token field;
  this task turns it mandatory per policy. If 0050B is ever activated, its
  console verbs are clients of the same gated ops — no second enforcement.
- Marker prefix follows the ops owners (`statefsd:`/`bootctld:`), not a
  `recovery:` pseudo-service.

## RFC seed landed 2026-08-24 (RFC-0088) — format decisions recorded there

- **Fixed 136-byte layout, NOT CBOR** (repo wire-format line; a CBOR dep is
  parser surface for nothing at this size).
- **Replay = per-(verifier, key) monotone high-water mark**, not a nonce
  index — bounded by construction, no GC, consume-before-act (a burned
  token is re-mintable, never a brick). The `/state/recovery/nonce.idx`
  wording below is superseded.
- **Trust anchor = build-time-baked `policies/nxra-trust.toml`** (image
  integrity is the honest bring-up root; TASK-0289 upgrades it). policyd
  keeps standing-capability authority — the paths compose.
- **Authorization model: standing capability OR valid token** — additive
  break-glass; the OTA ladder and admin subjects run untouched.
- **Time windows optional; `no-clock` = fail-closed reject** (recovery
  graph runs no timed).

## Context

Recovery operations are powerful (repair, slot switch, target changes). To make
"break glass" audited and bounded, mutating actions require a short-lived signed
authorization token with replay protection. Kernel unchanged; all enforcement in
userspace at the ops surface.

Host-first (format, sign/verify crate, `nx recovery token` helpers, host tests
— buildable today); OS-gated half lands on 0050/0051.

## Goal

1. `.nxra` token format (CBOR + Ed25519), versioned; **RFC seed first** (new
   artifact format = repo rule; registry already names `.nxra`).
2. Verification in the ops surface: required for mutating ops per policy;
   read-only ops (status/diag) exempt unless policy says otherwise.
3. Replay protection: nonce consumption in `/state/recovery/nonce.idx`
   (bounded, GC'd).
4. Policy integration: trusted pubkeys, per-subject action allowlist, lifetime
   bounds.
5. `nx recovery token make/show` (host).

## Non-Goals

- Kernel changes.
- A general-purpose auth framework — narrow: recovery action authorization.
- Remote/OTA token distribution.

## Constraints / invariants

- Deny-by-default for mutating recovery ops unless a valid `.nxra` is presented
  (once policy flips them mandatory).
- Deterministic verification, stable reject reasons (audit + tests).
- Bounded nonce index with GC; nonce consumption is transactional with the
  authorized action (both-or-neither — a consumed nonce without executed action
  must not brick the token holder; decide exact txn coupling in the RFC seed).
- Verification input is bounded before parse; identity for audit =
  kernel-attributed sender, token subject is authorization data only.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (key provisioning / trust root)**: where trusted pubkeys live and how
  they are provisioned (keystored record vs. policy bundle) — decide in the RFC
  seed; bring-up trust model labeled honestly (no hardware-root claims,
  TASK-0289 alignment note).
- **YELLOW (clock / time window)**: notBefore/notAfter need a time source; if
  wallclock is unreliable in the target graph, use a bounded boot-time-ns epoch
  and document limitations (timed/RFC-0076 exists in normal target; recovery
  graph may not run it).

## Stop conditions (Definition of Done)

### Execution recuts (2026-08-24, Option C)

- **Enforcement point v1 = bootctld only.** A statefsd fsck-repair token
  gate would be dead code today: the only fsck client (selftest) holds
  `statefs.admin` and standing always wins; no token-carrying client path
  exists. The shared `nxra` pipeline (incl. the fsck-repair action) is
  host-proven; the statefsd seam joins with the first real client
  (0050B console / nx device channel). `statefsd: nxra required` markers
  go with it.
- **No separate `nxra required` marker**: "required" IS the standing deny
  (`SELFTEST: nxra require ok` proves it end to end); an extra bootctld
  line would re-state the deny as noise.
- **Accept marker names the KEY, not a subject**: the token has no subject
  field (authorization data is the key + action; the audit subject is the
  kernel-attributed sender) — `bootctld: nxra accept (key=<id8>
  action=<label>)`.
- Format/replay/trust recuts (no CBOR; hwm instead of nonce index; baked
  trust) are recorded in the RFC-seed note above.

### Proof (Host) — required

`tests/nxra_host/`:

- sign/verify happy path; tamper detection; expiry/not-before enforcement;
  replay detection (simulated nonce store); `test_reject_*` for malformed CBOR,
  oversized token, unknown version, untrusted key, action-not-allowed.
  ✅ 2026-08-24 — 8 pipeline tests (simulated hwm store, consume-before-act
  crash semantics, per-verifier/per-key isolation, baked-anchor roundtrip)
  + 5 nxra unit tests (reject-order pins, window edges, keyid) +
  `nx recovery` CLI contract (4 tests). "malformed CBOR" reads
  "malformed fixed-layout" per the RFC recut.

### Proof (OS/QEMU) — gated on 0050/0051

- `bootctld: nxra accept (key=<id8> action=<label>)` ✅ (required marker)
- `bootctld: nxra reject (reason=<r>)` — stable reasons: malformed,
  unknown-version, untrusted-key, bad-signature, action-denied, replay,
  expired, not-yet-valid, no-clock. ✅ replay proven in-lane (required);
  the label set is pinned by host tests.
- `SELFTEST: nxra require ok` / `SELFTEST: nxra accept ok` /
  `SELFTEST: nxra replay deny ok` ✅ every proof boot (headless/full/smp1
  + reset boot 3), state-neutral by construction (authorized OP_SWITCH
  dies NotStaged at the machine); FAIL variants are fatal signatures.
  The hwm record is an Integrity envelope (`/state/boot/` floor);
  the probe derives its seq from the persisted hwm — monotone across
  keep-blk boots with no wall clock.

## Touched paths (allowlist)

- `userspace/nxra/` (new crate: parse/sign/verify)
- `source/services/bootctld/src/` + `source/services/statefsd/src/`
  (token gate on mutating ops)
- `tools/nx/` (`nx recovery token make/show`)
- `policies/` + `schemas/policy.config.schema.json` (trust + allowed actions)
- `tests/nxra_host/`
- `docs/rfcs/` (RFC seed, next free number)
- `docs/reliability/nxra.md`

## Plan (small PRs)

1. RFC seed (format, trust root, nonce-txn coupling) — approval zone.
2. Format + crate + host tests.
3. Policy integration (trusted keys, allowlists).
4. Ops-surface enforcement + OS selftests.
5. DevX (`nx recovery token`) + docs.
