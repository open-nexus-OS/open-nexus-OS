# ADR-0025: QEMU Smoke Proof Gating (Networking + DSoftBus)

Status: Accepted  
Date: 2026-02-04  
Owners: @runtime

## Context

Open Nexus OS follows a **host-first, QEMU-last** strategy. QEMU smoke runs are valuable for end-to-end wiring, but some proofs depend on QEMU backend behavior (e.g. slirp/usernet DHCP), which can be **environment-sensitive** and therefore flaky.

At the same time, the project needs deterministic smoke gates for:

- core bring-up (`init:*`, `*: ready`)
- MMIO capability distribution and virtio device bring-up
- minimal networking facade usability (`netstackd` interface configured)
- optional higher-level proofs (DHCP lease, gateway ping, DSoftBus OS transport)

## Decision

1) Keep the **canonical QEMU smoke harness** as `scripts/qemu-test.sh`, but treat some networking proofs as **optional, explicitly gated** requirements.

2) Make QEMU smoke validate **interface configuration** (via `net: smoltcp iface up ...`) as the default networking proof, instead of requiring slirp DHCP by default.

3) Add two environment-controlled proof gates:

- `REQUIRE_QEMU_DHCP=1`: require DHCP lease + dependent L3/L4 proofs
- `REQUIRE_DSOFTBUS=1`: require DSoftBus OS transport marker ladder

1) For deterministic single-VM smoke behavior when DHCP is unavailable/flaky, build `netstackd` in a **QEMU smoke compatibility mode** (`feature = "qemu-smoke"`) that uses a slirp/usernet-compatible static fallback IP:

- `10.0.2.15/24` (no gateway required for loopback-only bring-up)

The 2-VM harness remains the canonical proof for cross-VM networking and must not rely on slirp DHCP.

## Rationale

- **Determinism**: CI must not depend on slirp DHCP timing or host network behavior.
- **No fake success**: smoke markers remain tied to real behavior; optional proofs are still enforceable when needed.
- **Host-first**: on-wire DHCP invariants (ports/IPs) and state-machine integration are best validated by fast host tests.
- **Scope separation**:
  - Single-VM QEMU smoke proves “stack is usable” and “loopback scope works”.
  - 2-VM harness proves “real subnet datagrams + sessions work”.

## Consequences

- Default `just test-os`/`scripts/qemu-test.sh`:
  - requires `net: smoltcp iface up ...`
  - does **not** require DHCP or DSoftBus markers unless explicitly enabled
- Developers/CI can turn on stricter proofs when debugging or when the environment guarantees the backend:
  - `REQUIRE_QEMU_DHCP=1 ...`
  - `REQUIRE_DSOFTBUS=1 ...`
- `netstackd` gains a `qemu-smoke` feature used only by the QEMU smoke harness build.

## Invariants

- DHCP on-wire behavior must follow standards:
  - UDP **68 → 67**
  - initial IPv4 **0.0.0.0 → 255.255.255.255**
- Markers remain stable, deterministic strings.
- No success markers for stubbed behavior.

## How to use

### Default smoke (deterministic)

```bash
RUN_UNTIL_MARKER=1 just test-os
```

### Require DHCP proof (optional, environment-dependent)

```bash
REQUIRE_QEMU_DHCP=1 RUN_UNTIL_MARKER=1 just test-os
```

### Require DSoftBus proof (loopback scope)

```bash
REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 just test-os
```

### Host-first networking proof (fast)

```bash
cargo test -p nexus-net-os
```
