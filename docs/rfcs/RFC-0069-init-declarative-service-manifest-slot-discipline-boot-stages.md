# RFC-0069: Declarative service manifest for init — tiers, generic wiring, cap-slot discipline, boot stages

- Status: **In progress** (2026-07-02) — the vehicle for the init side of the boot track (user decision 2026-07-02: modernize nexus-init now instead of "own RFC later").
- Owners: @runtime
- Created: 2026-07-02
- Links:
  - Builds on: **RFC-0066** (the `ServiceSpec` spine + the generic wiring arm — this RFC grows that spine into the full manifest), orchestrator decomposition Stage 1 (phase modules) + Stage 2 (`CtrlChannel` routes array), ADR-0041 (atomic desktop reveal), the boot-track fixes (gpud reactive completion + splash pipeline, commits `27cb223d`, `84dd3749`).
  - Code: `source/init/nexus-init/src/service_topology.rs` (`ServiceSpec`, `SERVICE_SPECS`, `REQUIRED_ROUTES`), `source/init/nexus-init/src/bootstrap/wiring.rs` (`wire_services` — the ~20 bespoke arms + the generic arm), `source/init/nexus-init/src/bootstrap/endpoints.rs` (the `Endpoints` bag), `source/kernel/neuron/src/cap/mod.rs` (`CapTable::allocate`), `source/kernel/neuron/src/syscall/api.rs` (object-creation syscalls).
  - Related tracks: session management (login screen + real user session — the next theme; `sessiond` lands here as a manifest entry), SystemUI shell-config (profiles B/C dock onto the session stage).

## Problem — the 48th hand-written case

`wire_services` is a 1377-line match with ~20 bespoke arms. Each arm repeats the same four moves
with hand-picked inputs: transfer the service's pre-minted server endpoint pair, provision a
CAP_MOVE reply inbox, transfer send caps for its outbound routes, log. Every new service —
`sessiond` is next — means writing the next arm by hand. Three concrete failures come from this
shape today:

1. **Slot collisions kill service-side kernel objects.** Init wires a child's capability layout by
   `cap_transfer` (kernel picks the first free slot — deterministic only as long as nobody else
   allocates). Services are auto-resumed at spawn and run concurrently, so a service that creates
   its own kernel object early (`waitset_create` in policyd) takes the slot init's layout assumed →
   `capability-denied` → init aborts. Consequence: policyd cannot block on a waitset and instead
   runs a NONBLOCK sweep + yield loop — the last busy-poll on the critical grant path.
2. **Grants race the auto-resumed service.** `exec_v2` resumes children immediately; a driver that
   needs an MMIO/IRQ grant (hidrawd) spins until init gets around to it.
3. **Boot stages are implicit.** "Display is up", "the session may start", "the shell is visible"
   exist only as scattered markers. The upcoming login/session work needs a named, ordered contract
   to dock onto.

## Design

### 1. Cap-slot discipline (kernel): self-created objects allocate from the top

`CapTable` gains `allocate_high` — the same first-free scan, from the highest slot downward.
Syscalls that create a kernel object **for the calling task itself** (`waitset_create` first;
timers/fences/self-endpoints can follow) allocate high. Everything installed **into** a task by
someone else (exec-time installs, `cap_transfer`, CAP_MOVE deliveries, factory-created endpoints
for a target pid) keeps allocating low, first-free.

Result: init's deterministic layout owns the low range *by construction*, services own the high
range, and the entire collision class disappears — no coordination protocol, no ordering
requirement, no manifest field. This is the enabling primitive for policyd's waitset (and for any
future service that wants RFC-0033 objects at startup).

### 2. Server-pair array (finish what Stage 2 started)

The `Endpoints` bag holds ~45 individually named caps; the per-service server endpoint pairs move
into `server_pairs: [Option<CapPair>; ServiceId::COUNT]` (the exact shape Stage 2 gave
`CtrlChannel.routes`). Special capabilities (endpoint factory, MMIO windows, IRQ sources,
fixed-slot driver caps) stay named. This is what lets one generic path wire *pre-minted* pairs —
the current generic arm can only create *fresh* endpoints, which is wrong for any service whose
clients already hold the pre-minted send side.

### 3. The manifest: `ServiceSpec` grows into the per-service record

```rust
pub struct ServiceSpec {
    pub id: ServiceId,
    pub tier: Tier,                       // Display | Core | Background (spawn order + future staging)
    pub exposes_server: bool,             // transfer the pre-minted server pair
    pub reply_inbox: bool,                // CAP_MOVE reply inbox
    pub routes_to: &'static [ServiceId],  // send cap to each target's request endpoint
    pub stage: Stage,                     // Platform | DisplayReady | SessionStart | ShellVisible
}
```

`wire_from_spec` executes exactly the four moves for any regular service. A bespoke arm remains
only where a service is genuinely special (fixed-slot driver caps, MMIO/IRQ grants, selftest).
Adding `sessiond` = one `SERVICE_SPECS` entry, zero new orchestrator code.

Host tests extend the existing service_topology consistency checks: every `routes_to` appears in
`REQUIRED_ROUTES`; every spec'd service has a server pair; migrated services are not also bespoke.

### 4. Boot stages: the named contract the session track docks onto

Init emits ordered stage markers derived from the manifest: `stage: display-ready` (display +
input chain wired — the reveal is gpud's own contract per ADR-0041), `stage: session-start`
(where `sessiond` — and later the greeter/login — takes over), `stage: shell-visible`. Today the
stages fire in immediate succession (no behavior change); the session track replaces the
auto-transition at `session-start` with a real session manager without touching init again.

## Migration — per-service, boot-gated, delete-on-proof

Same discipline as Stages 1/2: each batch keeps riscv+host green, one boot per batch, markers
byte-identical (modulo the deleted bespoke log lines), then the bespoke arm + its
`is_bespoke_wired` entry are deleted.

1. **Batch K (kernel):** `allocate_high` + `waitset_create` switched to it. Gate: full proof boot
   (no `capability-denied`, ladder identical).
2. **Batch P (policyd):** waitset-blocking loop (the `bootstrap/responder.rs` pattern) replaces the
   3-slot NONBLOCK sweep. Gate: grants still flow, `spin_hz` ~0 idle.
3. **Batch 1..n (regular services):** rngd+timed → statefsd/samgrd/packagefsd/vfsd →
   bundlemgrd/netstackd/dsoftbusd → metricsd/logd/keystored/updated/execd. Drivers
   (gpud/windowd/inputd/hidrawd) + selftest-client stay bespoke until the grant story (below).
4. **Batch G (grants):** MMIO/IRQ grant declarations move into the spec for the drivers; ordering
   guarantee documented (grants issued in the wiring pass the service's first blocking recv waits
   on — removes hidrawd's startup grant-spin).
5. **Batch S (stages + sessiond):** stage markers + the `sessiond` manifest entry (skeleton
   service; owns `session-start`; today = immediate default-session handoff).
6. **Closure:** the boot-splash logo handover (task #122) — switch from the 2D text bootsplash
   into the pulsing wordmark splash earlier (candidates: render the wordmark in the 2D phase, or
   bring the GL splash up independently of the compositor handoff).

## Out of scope

Rewriting `run_bootstrap`'s linear setup (Stage 1b was deliberately skipped), the observability
event model (RFC-0068), QoS policy (evidence-gated separately after the boot-track re-measure),
and the login UI itself (only the seam ships here).
