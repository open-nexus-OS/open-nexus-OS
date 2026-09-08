# Network egress policy (TASK-0043, RFC-0091 `net.connect`)

## Model

- **Per-subject allow-list, default deny.** A subject's RFC-0091 profile
  (`policies/*.toml`, `[[abi_profile."<subject>".net.connect]]`) names the
  IPv4 CIDRs and port ranges it may connect to. No rule ⇒ every `connect` is
  refused. Precedence: longest CIDR, then narrowest port range, deny beats
  allow on ties (`docs/security/abi-filters.md`).
- **Decided by policyd, enforced at netstackd's facade.** netstackd carries
  the kernel-attributed sender into its `connect` handler and asks policyd
  `OP_ABI_EVAL` over init-wired fixed slots before any dial; a refusal
  answers wire `STATUS_DENY` and prints
  `!cap-deny: enforcer=netstackd class=net.connect dst=<ip>:<port>
  subject=0x<sid>`. Unattributed senders and an unreachable policyd fail
  closed; subjects without an authored profile are not governed yet
  (capability-only). Admitted tuples are cached per boot (bounded ring;
  refusals never).
- **Learn mode** (RFC-0091 §5/§6): an authenticated `OP_SET_ABI_MODE` lets
  refusals be collected (`abi.learn … class=net.connect arg=<ip>:<port>`)
  without changing the decision; `nx policy learn-gen` turns the log into a
  review-first rule skeleton (always the observed `/32`, never `0.0.0.0/0`).

## Audit and counters

- policyd audit record (logd scope `policyd.audit`): `audit v1 op=abi_eval
  decision=deny subject=0x<seam> reason=egress-denied` — the user-facing
  reason from the ONE deny taxonomy (`nexus_ipc::audit::DenyReason`:
  `policy`, `abi-rule:statefs`, `ingress-denied`, `egress-denied`,
  `abi-mode`, `quota-exceeded`).
- Counter `egress_denies_total{subject=0x<sid>}` (metricsd), counted in
  policyd where the decision is made, flushed at most once per second, at
  most 15 subject series + `subject=0xother` (cardinality cap under
  metricsd's `max_series_total`). `ingress_denies_total` and
  `quota_denies_total` follow the same shape.

## Boundary (RED, documented)

A subject holding `device.mmio.net` (today: `selftest-client`, for its
own smoltcp probes) drives the NIC directly and bypasses the facade —
the capability grant IS the boundary. Do not grant it to app subjects; the
sandboxing notes carry the same rule.

## Proofs

- Host: `cargo test -p security_v2_host` (`test_reject_egress_cidr`,
  `test_reject_egress_port`, `test_reject_unattributed_connect`, allowed
  passes, default deny), `cargo test -p nexus-ipc audit`
  (`test_reject_audit_reason_unknown`), `cargo test -p nexus-metrics deny`
  (tally cardinality + flush cadence).
- QEMU (headless/smp1): `net-egress: enforced (netstackd policy seam on)`,
  the two `!cap-deny … dst=…` lines, `SELFTEST: egress deny ok`,
  `SELFTEST: egress allow ok`, `SELFTEST: egress learn collected ok`.
