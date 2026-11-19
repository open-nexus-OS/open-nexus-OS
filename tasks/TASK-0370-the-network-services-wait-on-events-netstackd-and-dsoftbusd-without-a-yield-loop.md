---
schema: 1
id: TASK-0370
title: "The network services wait on events: netstackd and dsoftbusd without a yield loop — the device interrupt and timers wake them"
status: draft
owner: ["@runtime"]
created: 2026-11-11
updated: 2026-11-19
subsystem: networking
kind: service
novelty: variant
goal:
  metric: "yield call sites in netstackd and dsoftbusd; their yields per second on an idle network"
  baseline: "66 sites in netstackd and 34 in dsoftbusd after TASK-0363 P0 removed the device probe and the halt loop (counted 2026-11-11); on the board both are withheld (no net plane), on QEMU the facade and the DHCP wait spin on yield"
  target: "0 yield call sites in both; KINIT: yield top names neither in a QEMU lane's steady window"
  instrument: "grep over the two services (a retired-pattern gate) and the kernel's KINIT: yield top line"
consumer: hardware-network/N2
phases:
  - id: P0
    story: "the network owner sleeps until its device or a client has something for it: the virtio-net interrupt, its timer and its request endpoint are one waitset, the DHCP wait is a timer, and a client's blocking read is parked on the socket's readiness instead of a yield loop"
    state: planned
    gate: "QEMU: 0 yield_ call sites in source/services/netstackd (gate), KINIT: yield top does not name netstackd in the steady window, the net ladder (dhcp, icmp ping, tcp, udp) green"
    note: "2026-11-19 (for the network lane): just test-all stops at deny-check — eight new RustSec 'unsound' advisories for heapless 0.7.17 (RUSTSEC-2026-0574..0581: FindMutView::pop, Spsc::peek ordering, IndexMap::truncate, a panicking Drop, SortedLinkedList::zeroize, Deque TryFrom/make_contiguous, the treiber stack race). heapless comes only through vendor/smoltcp (a dev-dependency of userspace/nexus-net-os); config/deny.toml already carries the sibling atomic-polyfill exception with the same follow-up. Decide with this lane's netstackd work: replace smoltcp/heapless with a portable-atomic based stack (end form), or a dated deny exception per ID until then. Log: build/agents/test-all-2026-11-19.log."
  - id: P1
    story: "the distributed bus waits for its local address, its peers and its sessions as events: an address notification from the network owner, socket readiness and timers — no connect or discovery retry by yield"
    state: planned
    gate: "QEMU: 0 yield_ call sites in source/services/dsoftbusd (gate), the dsoftbus ladder green, KINIT: yield top does not name dsoftbusd"
---

<!-- `nx ledger new task` copies everything below this line; the frontmatter above shows the
     planning fields (RFC-0115). Write state with `nx ledger`, never by hand. -->

## The ask

Who asked, in their words, and the outcome they want.

## Measured before the cut

What the code and the system do today — numbers, log paths, the measurement folder. A plan
that names no measurement is a guess.

## Three lenses

- **Scope** — in: … · out: … (with where each "out" goes).
- **Invariant** — what must hold, and the `test_reject_*` that proves it.
- **Contract** — the RFC/ADR this implements or seeds.

## Not doing here

The scope firewall: what is deliberately left out, and which task or milestone step takes it.

## Definition of Done

- Every phase `done` with its evidence; the goal's target reached on the instrument.
- Host tests, the QEMU lanes and the board cycle the phases name are green.
- Security-relevant work has its `test_reject_*` and hardening markers
  (`docs/standards/SECURITY_STANDARDS.md`).
- Docs swept: CHANGELOG, affected docs, the RFC's status; `active_days` and, when the estimate
  missed, a `variance_note`.
