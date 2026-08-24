<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# `.nxra` — signed recovery action tokens (TASK-0053)

Status: shipped — bootctld enforcement QEMU-proven; format/pipeline host-tested.

## CONTEXT

- Scope: RFC-0088 break-glass on the recovery ops surface (TASK-0051):
  one 136-byte Ed25519-signed token authorizes exactly ONE mutating
  recovery action for a sender standing policy would deny.
- Contracts: `docs/rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md`
  (format, trust, replay, time model), `userspace/nxra/` (the ONE
  parse/verify/sign implementation).
- Proof commands: `cargo test -p nxra` + `cargo test -p nxra_host`,
  `cargo test -p nx --test recovery_cli`,
  `scripts/qemu-test.sh --profile=headless` (require → accept → replay
  chain, required markers).

## Model in one paragraph

Standing capability OR presented token — never both required, never
weakening: updated's OTA ladder and admin-capable subjects run untouched;
a token only ever authorizes what standing policy would have denied.
Trust is a build-time-baked key list (`policies/nxra-trust.toml` →
`nxra::BAKED_TRUST`; exactly as strong as image integrity — the hardware
root lands with TASK-0289). Replay protection is a per-(verifier, key)
monotone high-water mark persisted as an Integrity envelope at
`/state/boot/nxra.hwm.<keyid8>` BEFORE the action executes
(consume-before-act; a burned token is re-mintable, never a brick). Time
windows are optional; where no trusted wall clock exists (the recovery
graph runs no timed) a windowed token rejects `no-clock`, fail closed.

## Operator flow

```bash
# sign one action (seed file = 64 hex chars; seq: unix time works)
nx recovery token-make --key operator.hex --action target-set \
    --seq "$(date +%s)" --arg 1 --out token.nxra

# inspect any token + verdict against the baked anchor
nx recovery token-show token.nxra --json
```

The client appends the 136 token bytes to the mutating op frame
(`[B,T,1,op,arg,token…]`). Verifier markers carry the stable labels:
`bootctld: nxra accept (key=<id8> action=<label>)` /
`bootctld: nxra reject (reason=<malformed|unknown-version|untrusted-key|
bad-signature|action-denied|replay|expired|not-yet-valid|no-clock>)`.

## Proof chain (every proof boot)

`SELFTEST: nxra require ok` (standing deny holds without a token) →
`SELFTEST: nxra accept ok` (token authorizes an OP_SWITCH that the machine
then rejects `NotStaged` — state-neutral by construction) →
`SELFTEST: nxra replay deny ok` (the same token dies at the hwm). The
proof signer is the deterministic key from `policies/nxra-trust.toml`
(private half public by construction — proof images only; production
images replace the list).

## Deliberate limits (v1)

- Enforcement point is bootctld (target/slot/reset). The statefsd
  fsck-repair gate joins when a token-carrying client path exists —
  wiring it now would be dead code (the selftest holds `statefs.admin`;
  standing wins).
- No revocation list: trust changes ship as an image update.
- No device-side wall clock wiring: windowed tokens reject `no-clock`.
