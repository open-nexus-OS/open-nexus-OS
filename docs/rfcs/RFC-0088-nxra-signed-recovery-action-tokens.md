# RFC-0088: `.nxra` — Signed Recovery Action Tokens

- Status: Implemented (v1 — bootctld enforcement; statefsd seam recut to the first token-carrying client)
- Owners: @security @reliability
- Created: 2026-08-24
- Last Updated: 2026-08-24
- Links:
  - Tasks: `tasks/TASK-0053-recovery-signed-actions-nxra.md` (execution + proof)
  - Related RFCs: `docs/rfcs/RFC-0087-reliability-failure-model-v1.md` (§4 targets, §6 ops surface)
  - ADRs: `docs/adr/0055-bootctld-single-boot-state-authority.md`
  - Registry: `tasks/TRACK-AUTHORITY-NAMING.md` (artifact `.nxra`)

## Status at a Glance

- **Phase 1 (format + verify crate, host)**: ✅ (`nxra` 5 unit + `tests/nxra_host` 8 pipeline tests)
- **Phase 2 (trust anchor + policy schema)**: ✅ (`policies/nxra-trust.toml` → build-baked `BAKED_TRUST`)
- **Phase 3 (ops-surface enforcement + OS proofs)**: ✅ (bootctld; require→accept→replay chain gated every proof boot; hwm as Integrity envelope)
- **Phase 4 (`nx recovery token` DevX)**: ✅ (`token-make`/`token-show`, 4 CLI tests)

Wire-carriage open question resolved: inline token after the arg byte
(`[B,T,1,op,arg,token136]`; server inbuf 192).

## Scope boundaries (anti-drift)

- **This RFC owns**: the `.nxra` wire format, the verification contract
  (trust anchor, replay model, time model, stable reject reasons), and the
  authorization model on the recovery ops surface.
- **This RFC does NOT own**: the ops themselves (TASK-0051 wire), key
  *provisioning hardware* trust (TASK-0289), remote token distribution
  (non-goal), a general auth framework.

## Context

The recovery operations surface (TASK-0051) gates mutating ops on standing
capabilities (kernel-attributed sender + delegated policyd caps). That covers
the SYSTEM's own paths (updated drives the OTA slot machine; admin-capable
subjects run fsck repair). What is missing is audited, bounded **break-glass**:
an operator authorizing exactly one powerful action on one device without
holding a standing grant — signed offline, verified on device, replay-proof.

## Goals

- One versioned token format authorizing exactly ONE recovery action.
- Deterministic on-device verification with stable reject reasons.
- Replay protection that is bounded by construction (no GC obligations).
- Enforcement as an ADDITIVE authorization path on the existing ops surface.

## Non-Goals

- Kernel changes. Remote/OTA token distribution. General-purpose auth.
- Revocation lists (v1 trust changes ship as an image update — see Trust).

## Constraints / invariants (hard requirements)

- **Additive, never weakening**: a token can only authorize a request that
  standing policy would have denied; it never blocks the standing paths
  (`updated`'s OTA ladder and admin-capable subjects run untouched).
- **Deny-by-default**: no valid token + no standing grant = the existing
  deny (already proven: `SELFTEST: recovery ops deny ok`).
- **Bounded input**: fixed 136-byte token; parse rejects any other length
  before any field is read.
- **Audit identity**: the kernel-attributed sender is the audit subject;
  the token key identifies the AUTHORIZER — both are logged, neither is
  derived from payload strings.
- **Consume-before-act**: the replay mark persists BEFORE the action runs.
  A crash between consume and act burns the token but never bricks the
  holder — the holder owns the signing key and mints a fresh token.
- No `unwrap/expect` on token bytes; stubs never claim `ok`.

## Proposed design

### Token format v1 (normative, fixed layout — 136 bytes)

Deliberately NOT CBOR (departure from the pre-rewrite ledger text): every
at-rest/wire format in this repo is a bounded fixed layout (fsck report,
boot record, evidence slots, nexus-wire) — a CBOR dependency adds parser
surface for a 136-byte artifact and nothing else.

```
[0..4)    magic  "NXRA"
[4]       version = 1
[5]       action u8   (1=slot-switch, 2=target-set, 3=fsck-repair, 4=reset)
[6..8)    flags u16le (bit0 = time-window present; others must be 0)
[8..16)   seq u64le   (per-key monotone, chosen by the signer; unix-time
                       works — only ordering matters)
[16..24)  not_before_ns u64le  (0 unless flags.bit0)
[24..32)  not_after_ns  u64le  (0 unless flags.bit0)
[32..40)  arg u64le   (action argument: target/slot byte, 0 if unused)
[40..72)  signer public key (Ed25519, 32 bytes)
[72..136) Ed25519 signature over bytes [0..72)
```

Crypto: `ed25519-dalek 2` (`default-features = false, features=["alloc"]`
— already in the OS graph via the selftest; verification is pure, no RNG).
Key id for markers/records: first 8 hex chars of the public key.

### Trust anchor (Phase 2)

`policies/nxra-trust.toml` — per trusted key: pubkey (hex64) + allowed
action list. Baked at BUILD TIME into the `nxra` crate (build.rs codegen,
`markers_generated.rs` pattern) and linked by the verifiers. Rationale:
the trust list is a configuration-shaped SECURITY ANCHOR, not a runtime
policy decision — mutable-at-runtime trust would be pure attack surface
before verified boot exists. Honest label: the anchor is exactly as strong
as image integrity; the hardware root lands with TASK-0289. policyd stays
the authority for *standing* capabilities — the two paths compose, they do
not overlap.

### Replay protection: per-key high-water mark (no nonce index)

The pre-rewrite ledger's `/state/recovery/nonce.idx` (bounded, GC'd) is
recut: a consumed-nonce SET needs GC and forgets under wrap (= replay). A
per-(verifier, key) monotone HIGH-WATER MARK is bounded by construction
(one u64 record per trusted key per verifier) and needs no GC:

- accept requires `token.seq > hwm(verifier, key)`;
- consume persists `hwm := token.seq` BEFORE the action executes;
- storage: bootctld under `/state/boot/nxra.hwm.<keyid8>` (its existing
  `statefs.boot` grant), statefsd engine-direct under
  `/state/recovery/hwm.<keyid8>` (no wire, no new capability).

Cross-verifier replay is structurally dead: the action binds the verifier
(a bootctld action presented to statefsd rejects `action-denied` before
any hwm is consulted).

### Time model (recovery graph has no clock)

`timed` is not in the recovery core graph, so absolute windows are not
generally checkable. Deterministic rule: a token WITHOUT a window (flags
bit0 = 0) relies on the hwm one-shot alone; a token WITH a window is
enforced only where a trusted wall clock exists and is REJECTED
(`no-clock`, fail closed) where none does. The signer chooses the
trade-off; nothing is silently unchecked.

### Authorization flow at a mutating op

1. Standing gates run first (sender gate / delegated cap) — pass ⇒ done,
   token ignored.
2. Otherwise, if the request carries a token: bounded parse → version →
   trust lookup → signature → action+arg match the requested op → time
   window (if flagged) → hwm check → CONSUME (persist hwm) → execute.
3. Stable reject reasons (audited + marker):
   `malformed | unknown-version | untrusted-key | bad-signature |
   action-denied | replay | expired | not-yet-valid | no-clock`.

Markers (ledger contract): `bootctld: nxra accept (key=<id8> action=<a>)`,
`bootctld: nxra reject (reason=<r>)`, same shape with the `statefsd:`
prefix for fsck-repair; `SELFTEST: nxra accept ok` /
`SELFTEST: nxra replay deny ok` / `SELFTEST: nxra require ok`.

### DevX (Phase 4)

`nx recovery token make` (signs with a key file; host-only, uses the
dalek signing half) and `nx recovery token show` (decode + verdict against
a trust file). nx exit classes stay the CLI contract.

## Open questions (to close during Phase 1-3)

- Wire carriage: token appended to the existing op frames (bootctld
  `[B,T,1,op,arg,token136]`, statefs fsck `[S,F,ver,op,token136]`) vs. a
  dedicated `OP_PRESENT_TOKEN` — decide with the enforcement PR (frame
  size caps on both wires allow the inline form).
- Whether `reset` (action 4) stays token-gated only for non-standing
  senders or additionally requires a token even for standing ones in the
  recovery graph (RFC-0087 §4 leans "standing wins"; revisit with 0289).

## Proof gates

- Host (`tests/nxra_host/` + crate tests): sign/verify roundtrip, tamper,
  window edges, hwm replay, `test_reject_*` for every stable reason.
- OS (gated on the 0051 lane): accept/replay/require marker chain in the
  reset lane; `just test-all`.
