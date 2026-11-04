---
schema: 1
title: "Network ingress and egress policy"
role: reference
subsystem: security
sources: [source/services/ingressd, source/services/netstackd/src/os/facade/authz.rs, userspace/policy/src/expose.rs, userspace/policy/src/learn_gen.rs, tests/security_v2_host, tests/ingress_host]
contracts: [RFC-0091, RFC-0092, ADR-0061]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Network ingress and egress policy

> **Purpose** — who may connect out, who may listen on the NIC, where each decision is made and enforced, and how refusals show up.
> **Owns** — `source/services/ingressd/`, `source/services/netstackd/src/os/facade/authz.rs`, `userspace/policy/src/expose.rs`
> **Contracts** — [RFC-0091](../rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md) (`net.connect` / `net.bind` profiles), [RFC-0092](../rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md) (exposure contract, ingressd), [ADR-0061](../adr/0061-exposure-intent-instead-of-free-binds.md) (exposure is an intent, not a free bind)
> **Invariants** — default deny both ways. policyd decides, netstackd's facade enforces, the subject is the kernel sender. An unattributed sender or an unreachable policyd is refused. Holding `device.mmio.net` bypasses the facade, so the grant itself is the boundary.
> **Read next** — [abi-filters.md](abi-filters.md), [policyd.md](policyd.md), [sandboxing.md](sandboxing.md), [../network/authority.md](../network/authority.md)

## Model

| Direction | Rule source | Decided | Enforced | Refusal |
|---|---|---|---|---|
| Egress (`connect`) | `[[abi_profile."<svc>".net.connect]]` — IPv4 CIDR + port ranges | policyd `OP_ABI_EVAL` | netstackd facade, before any dial | wire `STATUS_DENY`, `!cap-deny: enforcer=netstackd class=net.connect dst=<ip>:<port> subject=0x<sid>` |
| Bind / listen | `[[abi_profile."<svc>".net.bind]]` — ports, `address = "loopback"` (default) or `"any"` | policyd `OP_ABI_EVAL` | netstackd facade | `!cap-deny: enforcer=netstackd class=net.bind port=<p> addr=<a> …`, audit `reason=ingress-denied` |
| Inbound exposure | `[[expose."<svc>"]]` in `policies/*.toml` + `net.expose` capability | ingressd (intent) + policyd (`net.expose`) | ingressd: CIDR filter, token bucket, relay to the loopback backend | `ingressd: deny (reason=…)`, counter `ingress_denies_total{subject}` |

- **Precedence**: longest CIDR, then narrowest port range; deny beats allow on ties ([abi-filters.md](abi-filters.md)).
- **Governed = authored**: a subject without an `[abi_profile]` is not argument-filtered; only its capabilities apply.
- **Caching**: netstackd caches admitted tuples per boot in a bounded ring and never caches refusals.
- **Wiring**: netstackd reaches policyd over init-pinned fixed slots, never routed from the hot loop. Once armed it prints `net-egress: enforced (netstackd policy seam on)`.

## Ingress: one gateway

- **Only `ingressd` may bind `address = "any"`.** The shared grammar refuses it for every other subject (`SchemaError::AnyBindNeedsGateway`). Everyone else's `net.bind` is loopback-only.
- **Exposure is an intent**: `[[expose."<subject>"]] {port, proto, cidr_allow, rate_per_s, burst, tls, backend}`. The declared subject registers it with ingressd at runtime. The kernel sender must be that subject, and policyd must confirm it holds `net.expose`; an unreachable policyd means refused.
- **Bounds** (`userspace/policy/src/expose.rs`): ≤ 8 exposures per subject, ≤ 8 CIDRs per exposure, ≤ 64 in total; `rate_per_s` ≤ 10000; `burst` ≤ 1000; `(port, proto)` unique across subjects.
- **TLS / mTLS** is a named slot that the grammar refuses (`tls != "none"`) until termination exists. No stub reports `ok`.
- **Facade support** the gateway relies on: in-facade loopback (`127/8` never touches the NIC), hairpin connects to the interface's own address, end-of-stream on peer close, and `OP_PEER_ADDR` for accept-side identity.
- **Data plane**: per exposure ≤ 16 stream links, 4 KiB windows and a per-turn byte budget; UDP has a peer table.
- **Grants**: ingressd holds `ipc.core` + `policy.delegate`. Its RFC-0091 profile is the one `any` bind plus `net.connect` to `127.0.0.0/8`.

## Egress: learn, then enforce

1. Switch the subject to Learn with an authenticated `OP_SET_ABI_MODE` (sender holds `policy.abi_mode`, epoch-guarded, never persisted).
2. Refusals keep failing, and each is also logged once: `abi.learn … class=net.connect arg=<ip>:<port> would=deny`.
3. Run `nx policy learn-gen <log> --subject <svc> --out <toml>`. The skeleton is review-first: a connect rule is always the observed `/32`, never `0.0.0.0/0`, and an `any` bind is commented out unless you pass `--allow-any`.
4. Review it, merge it into `policies/`, and bump `epoch`. Then run `nx policy validate --write-manifest`.

## Audit and counters

- policyd record on logd scope `policyd.audit`: `audit v1 op=abi_eval decision=deny subject=0x<seam> reason=egress-denied` (or `ingress-denied`). Reasons come from `nexus_ipc::audit::DenyReason`.
- Counters `egress_denies_total{subject=0x<sid>}` and `ingress_denies_total{subject}` are counted in policyd and sent to metricsd. `quota_denies_total` is counted in statefsd. Each flushes at most once per second and keeps at most 15 subject series plus `subject=0xother`.

## The raw-NIC boundary

netstackd holds `device.mmio.net` and drives the NIC; every policy check above sits in its facade. Any other holder of `device.mmio.net` bypasses all of it. Today that is `selftest-client`, for its own smoltcp probes. Never grant `device.mmio.net` to an app subject.

## Proof

- Host: `cargo test -p security_v2_host` (`test_reject_egress_cidr`, `test_reject_egress_port`, `test_reject_unattributed_connect`, `test_reject_nonloopback_bind_without_intent`); `cargo test -p ingress_host` (`test_reject_intent_policy_denied`, `test_reject_forged_intent_sender`, `test_reject_cidr`, `test_reject_rate_exceeded`, malformed frames); `cargo test -p nexus-ipc audit`; `cargo test -p nexus-metrics deny`.
- QEMU (phase `net`): `SELFTEST: egress deny ok`, `SELFTEST: egress allow ok`, `SELFTEST: egress learn collected ok`, `SELFTEST: ingress deny ok`, `SELFTEST: ingress allow ok`, `SELFTEST: ingress intent deny ok`, `SELFTEST: ingress cidr deny ok`, `SELFTEST: ingress rate ok`; each ingress result follows ingressd's own `ingressd: port open (…)` / `ingressd: deny (reason=…)` line.
