---
title: TASK-0323 Ingress gateway follow-ups: UDP data plane, facade listener-close op, TLS/mTLS termination slot
status: Draft
owner: @security
created: 2026-09-08
depends-on:
  - TASK-0052 # ingressd Layer A/B (Done 2026-09-08)
follow-up-tasks: []
links:
  - RFC: docs/rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md
  - ADR: docs/adr/0061-exposure-intent-instead-of-free-binds.md
  - Ingress overview: docs/security/ingress.md
  - Parent: tasks/TASK-0052-security-v3-ingress-policy-ingressd-gateway.md
---

# TASK-0323: Ingress gateway follow-ups (RFC-0092 open items)

## Why

TASK-0052 delivered the ONE inbound gateway end to end for TCP: declared
exposures, identity-bound intents, CIDR + rate at the accept side, bounded
relay to loopback backends, the facade loopback/hairpin + `OP_PEER_ADDR`
prerequisites, and the four QEMU proofs. Three contract items stayed open
by design (never stubbed, never `ok`-marked):

1. **UDP data plane.** `proto = "udp"` exposures compile and register (grammar,
   intent path, table) but the gateway binds/relays TCP only: a UDP intent
   answers `ingressd: port open FAIL (…, proto=udp)` + `STATUS_DENY
   reason=limit`. The relay bookkeeping (`ingressd::udp::PeerTable`) exists;
   the facade's UDP loopback is still the legacy port-keyed emulation
   (`LoopUdp`, ports 37020/34567/34569) and needs the same generalisation the
   TCP path got (127/8 binds = loopback sockets, send-to 127/8 / own address
   pairs with the local socket, `OP_UDP_RECV_FROM` reporting the pair's
   remote).
2. **Facade listener-close op.** `OP_UNEXPOSE` releases the gateway's links but
   the `0.0.0.0:<port>` listener stays bound (the facade has no listener-close
   op; `OP_CLOSE` is stream-scoped). Add `OP_LISTENER_CLOSE` (additive),
   drive it from `Event::Closed`, prove with a re-expose after unexpose.
3. **TLS / mTLS termination slot** (RFC-0092 §4): owner = network track;
   needs a `no_std` TLS fitting RFC-0009 dependency hygiene; certificates via
   configd/keystored; `nx policy validate --strict` review rule for
   `tls = "none"` on exposures that need confidentiality.

Also noted: the hairpin/loopback pair lives in the single facade process
(fine for the one-stack design; a second stack instance is out of scope).

## Packages

- **P0 — Contract**: RFC-0092 amendment (UDP relay semantics: datagram
  forwarding with the peer table for the reply path, bounds; listener-close
  op; TLS slot ownership). Approval: `docs/rfcs`.
- **P1 — netstackd UDP loopback generalisation** (`udp/bind.rs`,
  `send_to.rs`, `recv_from.rs`, `state.rs`): 127/8 + own-address pairing by
  port, remote reporting, host `test_reject_*` for bounds.
- **P2 — ingressd UDP data plane**: bind `0.0.0.0:<port>` (UDP), poll
  `recv_from`, CIDR + rate per datagram, forward to `127.0.0.1:<backend>`,
  reply path via `PeerTable`; markers `ingressd: port open (…, proto=udp)`,
  `SELFTEST: ingress udp allow ok`.
- **P3 — listener close**: `OP_LISTENER_CLOSE`, gateway `Event::Closed`
  wiring, `SELFTEST: ingress unexpose ok` (re-expose after unexpose works,
  a peer connect in between is refused).
- **P4 — TLS slot**: tracked with the network family (HOLD until the joint
  discussion).

## Stop conditions

Host: reject suites per package green. OS: the new markers gated in
headless/smp1 (proof-manifest + `scripts/qemu-test.sh` + docs together).
Docs + CHANGELOG + board; RFC-0092 open questions closed one by one.
