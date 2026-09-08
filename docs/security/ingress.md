# Inbound exposure (TASK-0052, RFC-0092)

Contract: `docs/rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md`;
decision: `docs/adr/0061-exposure-intent-instead-of-free-binds.md`.

- **Default deny.** Only the gateway (`ingressd`) may bind a non-loopback
  address; every other subject's `net.bind` is loopback-only and a bind to
  `any` is refused at netstackd's facade (`STATUS_DENY`, audit
  `reason=ingress-denied`, counter `ingress_denies_total{subject}`).
- **Exposure is an intent**: `[[expose."<subject>"]] {port, proto, cidr_allow,
  rate_per_s, burst, tls, backend}` in `policies/*.toml`, registered with the
  gateway at runtime by the declared subject (kernel identity), gated by the
  `net.expose` capability, fronted with a CIDR allow-list and a deterministic
  token bucket, forwarded to the subject's loopback backend.
- **TLS/mTLS** is a named slot; the grammar rejects it until the network track
  delivers termination — no stub ever reports `ok`.
- **Boundary**: raw-NIC subjects (`device.mmio.net`) bypass the facade; the
  grant is the boundary (`docs/security/sandboxing.md`).

Status: Phase 0 (contract) and Layer A (TASK-0052 P1) delivered 2026-09-08:
the shared grammar refuses `[[net.bind]] action = "allow" address = "any"` for
every subject but `ingressd` (`SchemaError::AnyBindNeedsGateway`), and the
selftest's bind to the NIC-facing address is refused at the seam
(`!cap-deny: enforcer=netstackd class=net.bind port=40000 addr=any …`,
`SELFTEST: ingress deny ok`).

Layer B host core (TASK-0052 P2, 2026-09-08): the `[[expose]]` grammar lives
in `userspace/policy/src/expose.rs` (bounds ≤ 8 per subject / ≤ 8 CIDRs /
≤ 64 total, `rate_per_s ≤ 10000`, `burst ≤ 1000`, `(port, proto)` unique
across subjects, `tls != "none"` refused) and is included by path into both
policyd's build (validation) and ingressd's build (`EXPOSE_ENTRIES`, the
data-plane table). `source/services/ingressd/` carries the identity-bound
intent registry (kernel sender must be the declared subject; policyd
`net.expose` via `IntentHost`, unreachable ⇒ refused), the CIDR accept
filter, the deterministic token bucket, the bounded stream relay (≤ 16 links
per exposure, 4 KiB windows, per-turn byte budget) and the UDP peer table.
Proof: `cargo test -p ingress_host` (allow end to end +
`test_reject_intent_policy_denied`, `test_reject_forged_intent_sender`,
`test_reject_cidr`, `test_reject_rate_exceeded`, malformed/unsupported frames,
IDL↔grammar↔wire pin, shipped-table consistency). The OS entry, init
wiring, grants and markers are P3; until then the crate carries no
`nexus-service` metadata and is not embedded.
