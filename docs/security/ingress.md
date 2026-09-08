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
`SELFTEST: ingress deny ok`). The gateway itself is P2/P3.
