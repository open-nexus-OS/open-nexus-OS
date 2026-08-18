# ADR-0057: Restarting a service re-provisions from declared topology only, and clients re-resolve via samgrd — restart is a protocol, not a respawn

- Status: Accepted
- Date: 2026-08-18
- Links:
  - Tasks: `tasks/TASK-0049B-service-supervision-v1.md` (execution + proof)
  - RFCs: `docs/rfcs/RFC-0087-reliability-failure-model-v1.md` (supervision contract),
    `docs/rfcs/RFC-0079-ipc-last-sender-eof.md` (PeerClosed trigger),
    `docs/rfcs/RFC-0069-init-declarative-service-manifest-slot-discipline-boot-stages.md`
    (ServiceSpec, slot discipline), `docs/rfcs/RFC-0066-production-grade-service-chain-declarative-routing-typed-ipc-inprocess-tests.md`
    (declarative routes, typed client), `docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md`
    (bounded retry semantics)
  - Related ADRs: `docs/adr/0055-bootctld-single-boot-state-authority.md`,
    `docs/adr/0056-task-exit-reason-kernel-abi.md`

## Context

In a capability microkernel, respawning a dead service is the easy half. The hard
half: every client holds capabilities to the dead instance's endpoints. Today
`samgrd` has no notion of stale entries, revocation, or re-resolve — a client that
cached a route keeps a cap to a corpse forever. Meanwhile the restart path itself
could become a rights-drift hole: whatever the new instance is handed, it holds —
if provisioning is ad hoc, five restarts can accumulate more authority than the
first boot had. The init ctrl-plane already mints slots/routes along a declarative
tree (RFC-0066/0069); restart must reuse exactly that machinery.

## Decision

- **Restart = derivation (no-rights-drift invariant).** A restarted instance is
  provisioned exclusively from the declarative topology
  (`service_topology::ServiceSpec` + `REQUIRED_ROUTES`) — the same path as first
  boot. No authority may be minted in the restart path that the topology does not
  declare. After N restarts, held rights and kernel resources are ≤ the state
  before the first restart (proved via the RFC-0013 resource sentinel under a
  restart storm).
- **samgrd learns staleness.** When a supervised service dies, the supervisor
  notifies samgrd; samgrd marks the service's registration **stale**. Resolves
  against a stale entry return a deterministic `Stale`-class error (ADR-0054: no
  wildcard) until the new instance re-registers, then resolves return the fresh
  endpoint.
- **Clients re-resolve, triggered by PeerClosed.** A client observing
  `PeerClosed`/EOF (RFC-0079) or a stale-resolve error drops its cached cap and
  re-resolves via its broker connection (RFC-0066 `Connection`), under RFC-0025
  bounded-retry semantics. Re-resolve is client-driven; there is no kernel-side
  cap rewriting in v1.
- **Persistent ctrl-plane slots are never closed.** Named routes and @reply slots
  of the init ctrl-plane are exempt from staleness — they are the rails the
  protocol itself runs on (existing hard rule, restated as part of this contract).
- Out of scope: kernel revoke trees (no kernel changes here), transparent cap
  migration, state handoff between instances (each service defines its own state
  recovery source per RFC-0087 §3).

## Consequences

- **Positive**: restart becomes safe-by-construction against rights drift;
  "cap to a corpse" gets a deterministic error instead of a hang; the recovery
  behavior of every client is testable in-process (RFC-0066 chain tests).
- **Negative / accepted cost**: samgrd grows a state (stale/fresh) and one
  notification edge from the supervisor; clients of critical services need the
  re-resolve handler (mostly in the shared `Connection` layer, not per client);
  services whose clients cannot cheaply re-attach (windowd surfaces) must define
  their re-attach depth in the ledger.
- **Follow-ups**: TASK-0049B (execution); TRACK-TIME-AS-RESOURCE notes that a
  future conserved-budget right would ride this same derivation tree.

## Alternatives considered

- **Kernel-side revoke + cap rewrite**: rejected for v1 — largest possible
  surface in an approval zone, and bounded revoke on a BKL kernel cannot honor
  the timing guarantee that would justify it.
- **Endpoint inheritance** (new instance adopts the dead instance's endpoints):
  rejected — hides death from clients, breaks kernel-attributed identity
  assumptions, and turns every restart into a silent impersonation.
- **No staleness; rely on retry timeouts alone** (RFC-0025 as-is): rejected —
  timeouts can't distinguish "slow" from "dead", so clients would hammer corpses
  and recovery latency would be the timeout ceiling, not the restart time.
