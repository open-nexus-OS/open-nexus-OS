# RFC-0066: Production-grade service chain — declarative capability routing, one typed IPC client, in-process chain tests

- Status: Draft (plan + phase-1 implementation)
- Owners: @runtime @ui
- Created: 2026-06-22
- Links:
  - Motivation: the v6b work (RFC-0065) repeatedly hit *boot-only* failures (abilitymgr crashed because init had no orchestrator arm for it; `X→bundlemgrd` needs hand-wired CAP_MOVE per client). These are **architecture smells**, not bugs.
  - Builds on: `source/init/nexus-init/route_table.rs` (typed routes), `userspace/nexus-ipc/reqrep.rs` (`ReplyBuffer`/`recv_match_until`/`loopback_channel`), `tools/nx` chain harness (`Contract`/`Hop`/`SimIpcBus`), `samgrd` (register/resolve).
  - ADR: `docs/adr/0017-service-architecture.md`, `docs/adr/0036-…service-split.md`.

## Problem — why we keep debugging at boot

Every time a service↔service link is added, three hand-written, boot-only things must line up:

1. **The init orchestrator** (`bootstrap/orchestrator.rs`) hand-wires every pair in a ~1500-line `match svc_name { … }` with bespoke `CtrlChannel` fields, endpoint creation, and cap transfers. **Forget an arm → the service boots, then crashes** (exactly the abilitymgr case: fell into `_ => {}`, no server endpoint, `recv error`, exit).
2. **The request/reply** is hand-rolled **per client** (rngd's ~90-line `policyd_allows` CAP_MOVE dance, copy-pasted for each new caller).
3. **Nothing is tested without QEMU** — "does X reach Y" is only knowable from a UART log.

This is a classic non-declarative, non-brokered, untyped chain. It is unstable by construction: the source of truth for *who-talks-to-whom* is scattered across imperative init code, and correctness is only observable at boot.

## What Apple, OHOS, and Fuchsia do

| Concern | Fuchsia | OpenHarmony | Apple | What we should adopt |
|---|---|---|---|---|
| Who-talks-to-whom | **Declarative capability routes** in component manifests (`.cml`), statically **route-validated** | samgr registry; abilities declare needed SAs | launchd plist + `MachServices`; bootstrap server | **One declarative route SSOT**, validated by a host test (no hand-wired god-match) |
| Getting a connection | Component manager hands you a typed **channel** to the capability | `samgr->GetSystemAbility(id)` returns a **proxy** | `xpc_connection_create_mach_service(name)` brokered by launchd | **Name-based brokered connection** via `samgrd` — clients ask by name, broker mints the channel |
| The wire | **FIDL** typed bindings (request/reply correlation, errors, generated) | IPC proxy/stub marshalling | XPC typed messages | **One reusable typed `Connection::call`** (correlation + reply built on `reqrep`), not per-client CAP_MOVE |
| Testing the chain | **Realm Builder** builds the topology in-process; protocols tested without booting | component/IPC unit tests | XPC unit tests | **In-process chain tests** (`tools/nx` `Contract`/`Hop` + `loopback_channel`) asserting hop order + route existence host-side |

The throughline: **declarative routing + a broker + one typed client + in-process testability.** We already have every primitive; they are just used inconsistently.

## Design — the production-grade chain (incremental, stable)

We do **not** rewrite the kernel IPC or rip out the working orchestrator in one shot (that would be the opposite of stable). We make the *source of truth declarative*, the *client reusable*, and the *chain host-tested*, then migrate consumers behind those seams.

### Invariants (the contract this RFC establishes)

- **One route SSOT**: the set of allowed `(from → to)` service routes is declarative data (`route_table::REQUIRED_ROUTES`), not implied by orchestrator arms. A host test fails if a declared route is not produced by the route builder, or a produced route is not declared.
- **One typed client**: cross-service request/reply goes through a single `nexus_ipc` connection abstraction (`Connection::call`) built on `reqrep` (nonce correlation + bounded reply). No service hand-rolls CAP_MOVE.
- **Brokered by name**: a client obtains a `Connection` by *service name* (resolved via `samgrd` on OS, `loopback_channel` in tests). No call site hard-codes slot numbers.
- **In-process tested**: every cross-service hop in a shipped path has a `tools/nx` contract + hop assertion that runs on the host — a route/protocol regression is a `cargo test` failure, not a boot crash.
- **Fail-closed, never brick**: a missing/declined route degrades to a typed error the caller handles; init wiring is best-effort and never aborts boot.

### Phase 1 (this RFC's implementation) — the seams, host-tested, zero boot risk

- **`nexus_ipc::Connection`** — one reusable typed request/reply client over a `Transport` trait (`loopback` for host tests, kernel CAP_MOVE for OS), built on `reqrep`. Host-tested via `loopback_channel`. This is the OHOS-proxy / Fuchsia-channel / XPC-connection equivalent.
- **`route_table::REQUIRED_ROUTES`** — the declarative route SSOT + a host test asserting the route builder and the SSOT agree (the Fuchsia "route validation" equivalent). Adding a service without its route is now a **host-test failure**.
- **`tools/nx` registry/lifecycle chain test** — an in-process `Contract`/`Hop` chain for `bundlemgrd → abilitymgr → windowd` asserting marker order (the Realm-Builder equivalent).

> **Finding (testability gap):** `nexus-init::route_table` is currently gated
> `#[cfg(all(feature = "os-payload", nexus_env = "os"))]`, so the declarative routing
> + `REQUIRED_ROUTES` can only be validated in the OS test config, not on the host.
> Making `route_table` host-compilable (it only needs `ServiceId`/`CapSlot`/`Rights`,
> all host-available) is part of Phase 2 — it is the concrete instance of "the chain
> isn't host-testable" this RFC fixes.

### Phase 2 — broker the connection through `samgrd`

`Connection::connect("bundlemgrd")` resolves via `samgrd` (register/resolve already exists) instead of orchestrator-pre-wired slots; clients stop caring about slot numbers. Migrate rngd/abilitymgr/windowd callers onto it.

### Phase 3 — derive init wiring from the declarative manifest

The orchestrator stops hand-wiring per service: it iterates `REQUIRED_ROUTES` + a per-service `ServiceSpec { exposes_server, reply_inbox, routes_to }` and provisions endpoints generically. The 1500-line `match` collapses to a data-driven loop. Forgetting a service becomes structurally impossible.

### Phase 4 — typed contracts end to end

Generate request/reply types from the IDL (`nexus-idl`/capnp) for the lifecycle/registry protocols so frames aren't hand-encoded; `Connection::call` becomes `proxy.method(args)`.

## Proof / validation

- **Host**: `Connection` loopback tests; `route_table` SSOT-vs-builder test; `tools/nx/tests/chain_app_lifecycle.rs`.
- **OS/QEMU**: the existing marker ladders (`abilitymgr: registry ok (n=…)`, etc.) — now also asserted in-process so a boot is *confirmation*, not *discovery*.

## Alternatives considered

- **Keep hand-wiring, just add the missing arm** — rejected: it fixes one instance of a structural bug class that will recur on every new service/route.
- **Full Fuchsia-style component framework now** — rejected for stability: too large a single change. We adopt the *principles* (declarative routes, broker, typed client, in-process tests) incrementally on the primitives we already have.

## Phasing summary

- **P1 (now)**: `Connection` (host-tested) + `REQUIRED_ROUTES` SSOT + test + nx registry chain test. Additive, zero boot risk.
- **P2**: broker connect via `samgrd`.
- **P3**: data-driven orchestrator from `ServiceSpec`/`REQUIRED_ROUTES`.
- **P4**: IDL-typed proxies.
