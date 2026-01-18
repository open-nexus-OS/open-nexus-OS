---
title: TRACK NexusNet SDK (cloud + DSoftBus): contracts + phased roadmap (local-first, deterministic, policy-gated)
status: Living
owner: @runtime @distributed
created: 2026-01-18
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Networking authority (canonical vs bring-up): docs/architecture/networking-authority.md
  - Userspace networking contract seed: docs/rfcs/RFC-0006-userspace-networking-v1.md
  - DSoftBus architecture: docs/adr/0005-dsoftbus-architecture.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - NexusMedia SDK track (optional large payload flows): tasks/TRACK-NEXUSMEDIA-SDK.md
  - Policy as Code (future unification): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - App capability matrix (future): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Networking step 1 (canonical): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - DSoftBus streams/mux: tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - DSoftBus localSim v1: tasks/TASK-0157-dsoftbus-v1a-local-sim-pairing-streams-host.md
  - DSoftBus OS wiring v1: tasks/TASK-0158-dsoftbus-v1b-os-consent-policy-registry-share-demo-cli-selftests.md
  - DSL service stubs (svc.*): tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
---

## Goal (track-level)

Deliver a first-party **NexusNet SDK** that enables:

- **local-first apps** (small/offline by default),
- optional **cloud scale** (sync/replication, GraphQL gateways, remote services),
- and safe, ergonomic access to **DSoftBus distributed features**
  without requiring every app developer to become a networking/security expert.

All while preserving Open Nexus OS invariants:

- **no ambient authority** (all network/distributed operations are capability-gated),
- **channel-bound identity** (trust kernel/transport identities, not payload strings),
- **bounded resources** (buffers, streams, retries, timeouts),
- **deterministic proofs** (host-first; OS/QEMU markers only after real behavior),
- **split control-plane vs data-plane** (small typed control; bulk out-of-band via filebuffer/VMO semantics).

## Scope boundaries (anti-drift)

- This is not “implement the whole internet”.
- Avoid inventing multiple competing stacks (authority is explicit; see networking-authority doc).
- Cloud protocols (GraphQL/etc.) are **optional** and should not become a hard OS dependency.

## Shared primitives

- **Transport facade**: small TCP/UDP surface (`nexus-net` style) with deterministic FakeNet backend for tests.
- **Identity binding**: remote peer identity must be tied to discovery mapping and Noise session (DSoftBus).
- **RPC surface**: typed IDL (Cap’n Proto) for control plane; versioned byte frames only as bring-up shortcuts.
- **Bulk transfer**: chunked, bounded transfers now; later map to true VMO/filebuffer semantics without API churn.
- **Retry + backoff**: deterministic and bounded (no unbounded exponential loops).
- **Error model**: stable codes + bounded “why”.

## Authority model (who owns what)

- **Networking device/MMIO ownership**: `netstackd` owns NIC/MMIO and exports a sockets facade via IPC (canonical path).
- **Distributed fabric**: `dsoftbusd` owns discovery/session/streams and exposes safe APIs to clients.
- **Apps** do not open device nodes or raw sockets directly; they use the SDK + service boundaries.
- **Policy**: `policyd` is the single authority for allow/deny and grants; runtime consent (if needed) is separate.

## Capability names (v0 catalog; stable strings)

### Local networking

- `network.tcp.connect`
- `network.tcp.listen` (system-only by default)
- `network.udp.send`
- `network.udp.bind` (system-only by default; apps usually go through higher-level services)

### DSoftBus (distributed)

- `dsoftbus.discover` (participate in discovery / see peers)
- `dsoftbus.session.open` (establish authenticated session)
- `dsoftbus.stream.open` (open a logical stream/channel)
- `dsoftbus.rpc.call` (invoke a remote service over the fabric)
- `dsoftbus.share.send` (send share payloads; typically needs consent + policy)
- `dsoftbus.share.receive`

### Cloud / sync (optional)

- `cloud.sync` (replication / sync engine)
- `cloud.graphql.query` (query only; bounded)
- `cloud.graphql.mutate` (mutations; policy-gated)

### HTTP / REST (optional but practical)

- `network.http.request` (HTTP client requests; bounded)

### OAuth2 / OIDC (optional but practical)

- `cloud.oauth2.start` (start an auth flow; typically interactive)
- `cloud.oauth2.finish` (finish callback/code exchange)
- `cloud.oauth2.token.refresh` (refresh tokens; background; policy-gated)
- `account.manage` (add/remove accounts; Settings/Accounts app only by default)
- `account.use` (use an existing account for a specific app/grant)

## Phase map

- **Phase 0 (host-first ergonomics)**
  - SDK can run entirely on host with deterministic FakeNet backends.
  - DSoftBus localSim is the reference for semantics and tests (no external sockets required).

- **Phase 1 (OS wiring, canonical path)**
  - `netstackd` sockets facade exists and is used by `dsoftbusd` (no direct MMIO in dsoftbusd).
  - DSoftBus sessions/streams are real on OS (bounded, deterministic markers).

- **Phase 2 (cloud scale + pro features)**
  - Optional sync engine(s) and GraphQL gateway(s) exist as services, not baked into every app.
  - Remote content/services can be exposed over DSoftBus with strict policy + audit.

## Local-first → cloud scale: design note (libsql + GraphQL)

This concept fits the track **if** we keep boundaries crisp:

- Local-first DB is an **app-level/library** concern (or a dedicated `contentd`-style service for shared data),
  not a kernel concern.
- Cloud sync / GraphQL are **optional services** with explicit capabilities and deterministic test harnesses.
- The DSL should prefer **typed stubs** (svc.*) and structured query objects over “stringly GraphQL everywhere”.

## DSL surface v0 (proposed; ergonomic + safe defaults)

These are the **user-facing** DSL calls we should aim for. They hide transport details while enforcing:

- explicit identity binding (where applicable),
- bounded retries/timeouts,
- deterministic behavior for tests (via host FakeNet/localSim),
- policy-gated operations (capabilities).

### `svc.bus.*` (DSoftBus)

- `svc.bus.discover(filter)`  
  - **cap**: `dsoftbus.discover`  
  - **defaults**: bounded result count; bounded time window; deterministic ordering
- `svc.bus.pair(peer)`  
  - **cap**: `dsoftbus.session.open` (+ runtime consent where required)  
  - returns a `Session`
- `svc.bus.call(session, service, method, request)`  
  - **cap**: `dsoftbus.rpc.call`  
  - typed stubs preferred; enforce timeout + max response bytes
- `svc.bus.openStream(session, purpose)`  
  - **cap**: `dsoftbus.stream.open`  
  - returns a `Stream` with bounded send/recv windows
- `svc.bus.send(stream, bytes)` / `svc.bus.recv(stream)`  
  - bounded chunk sizes; backpressure surfaced as explicit errors

### `svc.cloud.*` (sync + GraphQL gateways)

- `svc.cloud.sync.pull(dataset)` / `svc.cloud.sync.push(dataset)`  
  - **cap**: `cloud.sync`  
  - deterministic retry budget; conflict policy is explicit (no silent merges)
- `svc.cloud.graphql.query(endpoint, queryObj)`  
  - **cap**: `cloud.graphql.query`  
  - prefer structured/typed query objects, not raw strings
- `svc.cloud.graphql.mutate(endpoint, mutationObj)`  
  - **cap**: `cloud.graphql.mutate`

### `svc.net.*` (HTTP/REST)

- `svc.net.http.request(req)`  
  - **cap**: `network.http.request`  
  - enforce: method allowlist, max headers, max body bytes, timeouts, redirect policy

### `svc.auth.*` (OAuth2/OIDC)

- `svc.auth.oauth2.start(provider)`  
  - **cap**: `cloud.oauth2.start`  
  - returns an auth handle; UI/consent integration is explicit
- `svc.auth.oauth2.finish(handle, callback)`  
  - **cap**: `cloud.oauth2.finish`
- `svc.auth.oauth2.refresh(handle)`  
  - **cap**: `cloud.oauth2.token.refresh`

## Note: REST + OAuth2 as a “bridge” across Web/Desktop

Yes: a **native** REST + OAuth2/OIDC API is a good complement, because it:

- avoids every app bundling its own HTTP/OAuth stack (attack surface + inconsistency),
- gives you a single place for policy enforcement, timeouts, budgets, and safe defaults,
- makes DSL ergonomics consistent across “local app”, “cloud app”, and “distributed app”.

Important guardrails:

- OAuth tokens are **secrets**: never logged; storage is mediated by identity/keystore services.
- “Raw HTTP everywhere” is risky: encourage typed service stubs and structured query objects in DSL; keep HTTP as an escape hatch with strict bounds.

## Account-based sign-in (iOS-style UX, capability-style security)

Your idea (“add Google account once in Settings; Calendar/Mail reuse it”) is a great UX pattern **if** we
avoid “global tokens for all apps” and instead do **per-app grants**.

### Recommended model (high-level)

- **Accounts UI (System/Settings)** adds an account (Google today, Nexus account later).
  - This UI is the only place that runs interactive OAuth2 flows by default.
- A dedicated authority (e.g. `authd` or `identityd` extension) holds **refresh tokens** in the per-user vault
  (keychain/keystore) and never exposes them to apps.
- Apps (Calendar/Mail) request **a scoped grant** (“this app may access account X with scopes S”) which is:
  - policy-gated (`policyd`) and typically requires a user prompt (perms/consent),
  - revocable in Settings,
  - auditable.
- Network calls then use a **token handle** / **session-bound credential**:
  - either the HTTP client (`svc.net.http.request`) can attach auth automatically by handle,
  - or typed service stubs call an internal gateway service that handles tokens on behalf of the app.

### Why this is safer than “global tokens”

- **Least privilege**: Calendar doesn’t automatically inherit Mail’s permissions/scopes.
- **Revocation**: remove account or revoke one app’s grant without breaking others.
- **Reduced secret exposure**: apps don’t handle refresh tokens; fewer leak paths.
- **Consistent enforcement**: budgets/timeout/audit are applied in one place.

### DSL ergonomics v0 (shape)

- `svc.auth.account.list()` → list accounts visible to the app (filtered by grants)
- `svc.auth.account.requestAccess(account, scopes)` → triggers consent + records a grant
- `svc.net.http.request({ ..., auth: { account, scopes } })` → attaches token implicitly (no raw token strings)

## Pluggable account providers (ecosystem-friendly, still safe)

Instead of a fixed “built-in” set of global account types, the OS can support **pluggable account providers**
that can be installed/updated like other bundles. This enables third parties to ship a provider once, and then
multiple apps can reuse it (social app now, chat later, photo editor later) without each app re-implementing OAuth.

### Provider model (recommended)

- **Provider bundle** (signed, installable) registers an `account_provider_id` and declares:
  - supported auth protocol(s): OAuth2 / OIDC / custom,
  - supported scope families and UX strings,
  - callback/redirect handling requirements,
  - minimum policy requirements.
- The **Accounts UI** enumerates installed providers and runs the provider-owned auth flow in a controlled surface
  (no raw embedded webview by default; strict redirect + CSP rules).
- The **auth authority** holds refresh tokens for `(user, provider, account)` and issues **short-lived access tokens**
  on demand, returning only **handles** to apps (or attaching tokens internally in `svc.net.http.request`).
- Apps request grants against `(provider, account, scopes)`; grants remain **per-app** and revocable.

### Security guardrails (non-negotiable)

- Providers must be **signed** and policy-approved to register globally (installer + policyd gates).
- Tokens are secrets: never logged; stored only in keystore/keychain namespaces.
- No “global token visibility”: apps never enumerate or read tokens; they only get scoped access.
- Provider UX must prevent phishing:
  - strict redirect allowlists,
  - bounded timeouts,
  - clear UI indicators (“You are signing into <provider>”),
  - deny arbitrary URL navigation by providers unless explicitly allowed.

### Capability sketch (v0)

- `account.provider.register` (register a provider; system/installer only)
- `account.manage` (add/remove accounts; Settings/Accounts app only by default)
- `account.use` (use an existing account via a per-app grant)

## Extraction rules

A candidate becomes a real `TASK-XXXX` only when it:

- proves bounded, deterministic behavior on host tests,
- does not introduce a second networking/distributed authority,
- documents identity and policy invariants (DON’T trust payload identity),
- keeps cloud pieces optional and capability-gated.
