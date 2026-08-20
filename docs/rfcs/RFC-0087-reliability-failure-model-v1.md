# RFC-0087: Reliability & Failure Model v1 — supervision, exhaustion events, degradation, boot targets

- Status: Draft
- Owners: @runtime / @reliability
- Created: 2026-08-18
- Last Updated: 2026-08-18
- Links:
  - Tasks: `tasks/TASK-0049-fault-exhaustion-truth-proof-reanimation.md`,
    `tasks/TASK-0049B-service-supervision-v1.md`,
    `tasks/TASK-0049C-persistent-evidence-journal.md`,
    `tasks/TASK-0050-system-reset-boot-targets-bootctld.md`,
    `tasks/TASK-0051-recovery-operations-surface.md`,
    `tasks/TASK-0051B-crash-evidence-at-rest.md`,
    `tasks/TASK-0053-security-v3-signed-recovery-actions-nxra.md`
  - ADRs: `docs/adr/0055-bootctld-single-boot-state-authority.md`,
    `docs/adr/0056-task-exit-reason-kernel-abi.md`,
    `docs/adr/0057-service-restart-capability-re-resolve.md`
  - Related RFCs: `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md`
    (readiness vocabulary), `docs/rfcs/RFC-0069-init-declarative-service-manifest-slot-discipline-boot-stages.md`
    (ServiceSpec, boot stages), `docs/rfcs/RFC-0079-ipc-last-sender-eof.md`
    (PeerClosed signal), `docs/rfcs/RFC-0081-process-reaper-nonblocking-reclaim.md`
    (reap mechanics), `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (crash envelope)
  - Track: `tasks/TRACK-TIME-AS-RESOURCE.md` (conserved time/budget rights — future)

## Status at a Glance

- **Phase 0 (Detection: exit reasons + exhaustion events)**: ⬜ — TASK-0049
- **Phase 1 (Supervision: restart/backoff/crash-loop/re-resolve)**: ⬜ — TASK-0049B
- **Phase 2 (Evidence: persistent journal + crash at rest)**: ⬜ — TASK-0049C, TASK-0051B
- **Phase 3 (Boot state: reset + targets)**: ⬜ — TASK-0050
- **Phase 4 (Recovery operations + authorization)**: ⬜ — TASK-0051, TASK-0053

Definition: "Complete" means the contract is defined and the proof gates are green.

## Scope boundaries (anti-drift)

- **This RFC owns**:
  - the failure vocabulary (exit reasons, exhaustion events, degradation states),
  - the supervision contract (criticality tiers, restart/backoff/crash-loop policy),
  - the degradation contract for critical services,
  - the boot-target semantics (`normal | recovery | safe`),
  - the evidence retention principles.
- **This RFC does NOT own**:
  - kernel memory accounting/pressure/OOM enforcement (TASK-0286/TASK-0287 — this
    RFC only defines the *event vocabulary* those emit into),
  - the exit-reason ABI encoding (ADR-0056 decides it; this RFC consumes it),
  - boot-state record layout and ownership (ADR-0055),
  - the capability re-resolve wire protocol (ADR-0057),
  - update/OTA slot semantics (RFC-0012 / TASK-0036 own the slot machine;
    this RFC only names who holds the record — ADR-0055),
  - time/budget capabilities (TRACK-TIME-AS-RESOURCE; deliberately not a task yet).

### Relationship to tasks (single execution truth)

Tasks named in Links define stop conditions and proofs. This RFC changes only via
PR that also updates the affected task ledgers.

## Context

The repo has a strong *storage* reliability floor (statefs journal v2 + crash
injection + fsck, nxfs), a strong *boot diagnostics* floor (RFC-0013), and a good
*crash forensics host toolchain* (RFC-0031, `.nxcd`/`nxsym`/`nx crash`). Between
"a process dies" and "the system recovers" there is nothing:

- No supervision on the OS: `execd::os_lite::exec_elf(…, _restart)` ignores the
  restart policy (`Unsupported`); `init: supervise X restart=always` is a host-side
  `println!`. Nobody observes init-spawned services dying.
- Every fault is `exit(-22)` — indistinguishable from a regular exit.
- No reset path exists (no SBI SRST caller anywhere).
- logd is a RAM ring (128 records / 16 KiB); crash records do not survive reboot.
- Silent degradation is real and documented: VMO-arena exhaustion silently drops
  gpud from GL to 2D; statefsd silently stays RAM-backed when the virtio upgrade
  window is missed.
- RFC-0002 lists "failure handling" as an open question; it was never answered.
  `docs/services/lifecycle.md` describes a supervisor that does not exist on OS.

This RFC is the missing contract. It also encodes three conservation principles
inspired by capability-time systems (S3K, RTSS 2025; correspondence with
R. Guanciale 2026-08) — as *contract properties*, not as the slice mechanism,
which does not fit our EDT/SMP direction (ADR-0052).

## Goals

- Every service death has a knowable reason; every resource exhaustion is an event.
- Critical services restart automatically under a bounded, deterministic policy;
  crash loops are capped and survive reboot.
- Clients recover their capabilities to a restarted service via a defined protocol.
- The system always boots into *something usable* (`normal | recovery | safe`).
- Evidence (crash, degradation, audit) survives reboot within bounded budgets.

## Non-Goals

- Perfect fault capture for uncontrolled crashes without kernel debug support
  (RFC-0031 limitation stands).
- A general watchdog for *hangs* (liveness beyond ready-marker + PeerClosed is a
  follow-up; v1 detects exits and exhaustion, not silent wedges).
- Time/budget capabilities (TRACK-TIME-AS-RESOURCE).
- An interactive recovery shell (ledger exists, deferred: TASK-0050B).

## Constraints / invariants (hard requirements)

- **Determinism**: backoff schedules and crash-loop windows use injectable time
  sources in tests; no wall-clock flakes.
- **No fake success**: degradation and recovery markers only after real behavior.
- **Bounded resources**: crash history, evidence spill, and dump retention all
  carry explicit byte/record budgets.
- **Security floor**: supervision decisions bind to kernel-attributed identity
  (`sender_service_id`), never payload strings; recovery mutations are policy-gated
  (deny-by-default) and — where required — `.nxra`-authorized (TASK-0053).
- **Stubs policy**: absent capabilities say `stub`/`placeholder`, never `ok`.

## Proposed design

### 1. Failure vocabulary (normative)

**Exit reason** (ADR-0056 encodes; this RFC fixes the taxonomy):

| Reason | Meaning |
|---|---|
| `clean` | voluntary `exit(0)` |
| `error` | voluntary `exit(!=0)` |
| `fault{cause}` | kernel killed the task on a trap (cause from the trap ring) |
| `panic` | runtime panic path (service entry marks before exit) |
| `kill{by}` | killed by an authority (policy, OOM handoff, supervisor) |

**Exhaustion event**: every budget, arena, quota, or capacity that can run out has
a *defined exhausted moment* that emits an event (logd record, `event=exhaust.v1`,
bounded fields: `resource`, `owner`, `action_taken`) plus a deterministic marker.
Silent fallbacks are a contract violation of the same class as fake-green markers.
First two enforcements (TASK-0049): `gpud: degrade gl->2d (reason=arena)` and
`statefsd: degrade ram-backed (upgrade missed)`.

**Degradation state**: a service that continues in a reduced mode is *degraded*,
must say so once (marker + event), and must report the state on query. Degraded is
a legal, visible state — never a secret.

### 2. Supervision contract (normative)

- **Policy lives in the declarative topology** (RFC-0069 `ServiceSpec`):
  `criticality` tier + `restart` policy + backoff parameters. Policy is data,
  not code. `nexus-init` owns the policy; `execd` remains spawn/reap/report
  mechanics (RFC-0081).
- **Criticality tiers**:
  - `critical-boot` (statefsd, samgrd, policyd, execd, logd): failure at boot is a
    boot failure; failure at runtime triggers restart with highest urgency; repeated
    failure escalates to reboot-into-`safe` (via ADR-0055 record).
  - `critical-session` (windowd, gpud, vfsd, inputd, settingsd): restart with
    backoff; session surfaces re-attach via ADR-0057; repeated failure degrades the
    session (defined per service, e.g. windowd restart ⇒ clients re-present).
  - `standard` (everything else): restart per policy; crash loop parks the service
    (`blocked`) without harming the system.
- **Backoff + crash-loop cap**: exponential backoff (`initial_ms`, `factor`,
  `max_ms`), crash-loop window + threshold (N crashes in W seconds ⇒ `blocked`,
  marker `init: crash-loop blocked svc=<name> reason=<reason>`). Counters persist
  as a bounded statefs record so loops survive reboot. Decisions consume the real
  exit reason — a `clean` exit never counts toward a crash loop.
- **Restart = derivation (no-rights-drift)**: re-provisioning draws exclusively
  from the declared topology; no authority is created in the restart path. After N
  restarts, held rights and kernel resources are ≤ the state before the first
  (proved via the RFC-0013 resource sentinel under restart storm).
- **Supervision path has right-of-way**: supervisor/reaper/evidence paths run under
  the existing QoS/lockclass right-of-way (ADR-0049/ADR-0052). Upgrading this
  policy to a conserved time/budget *right* is the named end state, owned by
  TRACK-TIME-AS-RESOURCE — not by any task in this lane.

### 3. Degradation contract for critical services (normative)

For each `critical-*` service the ledger implementing supervision must state:
what clients observe during death (RFC-0079 PeerClosed / RFC-0025 bounded retry),
what the restarted instance guarantees (state recovery source), and what the
degraded mode is if restart is exhausted. No critical service may have an
undefined death behavior.

**Client re-attach contract** (implemented by TASK-0049B; proven by the
selftest restart storm):

1. **Death window.** Between service exit and supervised respawn, the
   service's request endpoint is gone (kernel closes owner-bound endpoints
   with the task). Clients observe send/recv errors or `PeerClosed` — never
   a hang: every client wait is bounded (RFC-0025).
2. **Staleness is answered, not guessed.** The resolve authority (init
   responder + RouteTable; ADR-0057) marks the dead service's route stale
   and answers route requests with `STATUS_STALE` until the respawn has
   re-provisioned the endpoint. Clients must treat `STATUS_STALE` as
   "retry bounded", not as `NOT_FOUND`.
3. **Re-resolve, then re-attach.** Recovery is always: re-resolve the route
   (bounded retry budget), obtain fresh slots, resume the protocol from a
   client-side known state. Session state held by the dead instance is gone
   unless the service's ledger names a recovery source (statefs record,
   re-subscription, replay).
4. **Slot hygiene is part of the contract.** After a successful re-resolve,
   the client closes its previous send-slot cap. A client that hoards caps
   to dead endpoints across N restarts violates the no-rights-drift
   invariant from the consumer side (resources must stay flat under a
   restart storm — the resource sentinel is the gate).
5. **Persistent ctrl-plane slots are exempt.** init-owned ctrl channels and
   `@reply` slots survive the restart by construction and are NEVER closed
   or re-minted per restart; only the service's request endpoint cycles.

### 4. Boot targets (normative)

- `boot_target ∈ {normal, recovery, safe}` lives in the ADR-0055 record
  (`bootctld`), never in a second store, never in a boot-arg parser in init.
- Targets select a *declarative stage graph* in the init manifest (RFC-0069
  stages). `recovery` = minimal graph (statefsd, logd, policyd, bootctld + ops
  surface). `safe` = session graph minus non-essential services, conservative
  profiles. No separate recovery binary, no second init.
- Reset (SBI SRST) is the only transition mechanism; `next_boot` is consumed
  exactly once (one-shot semantics).

### 5. Evidence retention (normative)

- Evidence classes: crash records (RFC-0011 envelope + `reason`), exhaustion
  events, audit records. All spill from logd's RAM ring to statefs under
  `/state/logd/` with byte budgets and drop-oldest semantics (TASK-0049C).
- Crash dumps at rest use the `.nxcd.zst` container (authority registry) with
  TTL + `max_bytes` GC (TASK-0051B). Bulk placement must respect ADR-0043
  (`/state` is a KV, not a file dump).
- Never persist secrets; `/state/secret/*` never enters evidence regardless of
  policy.

## Security considerations

- **Threat model**: a malicious/compromised service forcing restarts to exhaust
  resources or escalate rights; forged crash metadata; evidence exfiltration;
  recovery operations abused as a side door.
- **Mitigations**: no-rights-drift invariant (restart cannot mint); crash-loop
  caps; kernel-attributed identity for all supervision decisions; policy-gated +
  `.nxra`-authorized recovery mutations; conservative redaction defaults.
- **DON'T DO**: don't let a restarted instance inherit slots the topology doesn't
  declare; don't emit `ready` for a degraded service without the degradation
  event; don't treat `exit(0)` as crash; don't build a second supervisor.

## Failure model (normative)

This RFC *is* the failure model; see sections 1–5. Meta-rule: recovery actions are
themselves bounded (a repair/GC/restart that can run unboundedly merely relocates
the outage).

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-init  # topology/backoff/crash-loop
cd /home/jenning/open-nexus-OS && cargo test -p execd       # exit-reason plumbing
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (introduced across TASK-0049..0051B)

- `execd: exit pid=<pid> reason=<reason> code=<code>`
- `gpud: degrade gl->2d (reason=arena)` / `statefsd: degrade ram-backed (upgrade missed)`
- `init: restart svc=<name> attempt=<n>` / `init: crash-loop blocked svc=<name> reason=<reason>`
- `bootctld: ready` / `bootctld: target=<t> next=<t>`
- `SELFTEST: supervision restart ok` / `SELFTEST: crash-loop cap ok`
- `SELFTEST: evidence persist ok`

Standing fault injector (ADR-0048 doctrine): the selftest kills a live service and
asserts restart + re-resolve + degradation markers; the detector is the regression
test.

## Alternatives considered

- **Per-topic mini-RFCs** (supervision, degradation, boot targets separately):
  rejected — fragmenting this contract is exactly what produced four competing
  boot-state authorities and three diag-bundle formats.
- **execd as supervision policy owner**: rejected — policy belongs in the
  declarative manifest (RFC-0069); execd stays mechanics (user decision 2026-08-18).
- **Adopting S3K slice scheduling**: rejected for the mechanism (conflicts with
  ADR-0052 EDT/affinity and dynamic UI load); the conservation principles are
  adopted as contract properties instead; mechanism study lives in
  TRACK-TIME-AS-RESOURCE.

## Open questions

- Liveness beyond exits (hang detection / health probes): follow-up RFC once
  supervision v1 is proven; candidates: bounded ping over existing control planes.
- windowd/gpud client re-attach depth (full scene rebuild vs. surface re-present):
  decided in TASK-0049B ledger per ADR-0057 protocol.

---

## Implementation Checklist

- [ ] **Phase 0**: exit reasons + exhaustion events + proof reanimation — TASK-0049
- [ ] **Phase 1**: supervision v1 — TASK-0049B
- [ ] **Phase 2**: evidence journal + crash at rest — TASK-0049C, TASK-0051B
- [ ] **Phase 3**: reset + boot targets — TASK-0050
- [ ] **Phase 4**: recovery ops + `.nxra` — TASK-0051, TASK-0053
- [ ] QEMU markers appear in `scripts/qemu-test.sh` + proof-manifest and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
