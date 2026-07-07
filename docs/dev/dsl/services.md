<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Services & Effects

Service calls are performed from **effects**, not reducers.

## Rule: reducers are pure

- no IO
- no `svc.*`
- no query execution (`match SomeQuery(…)`)
- no DB/files

The checker enforces this (`NX0405 ReducerImpure`).

## The service surface (generated, never hand-maintained)

The set of services and methods the DSL may call is defined in **one** schema:
`tools/nexus-idl/schemas/dsl_services.capnp` (`const dslSurface`). It is part
of the platform IDL directory every service speaks from. From this single file:

- the **frontend signature table** is generated at build time
  (`nexus-dsl-core`'s `build.rs` → `registry::SVC_SURFACE`), so the checker is
  structurally unable to disagree with the platform surface;
- the app-host routes `svc.*` calls against the same list (TASK-0080D).

Signature diagnostics (stable codes):

| Code | Meaning |
| --- | --- |
| `NX0207` | `svc.<service>` is not a platform service |
| `NX0208` | the service exists but has no such method |
| `NX0302` | wrong argument count (excluding `timeoutMs:`) |
| `NX0409` | missing explicit `timeoutMs:` (warning in v0.2) |
| `NX0407` | a call result is ignored (warning in v0.2) |

## Calling a service

```nx
@effect on LoadRequested {
    match svc.library.list(timeoutMs: 250) {
        Ok(rows) => dispatch(Loaded(rows)),
        Err(e) => dispatch(LoadFailed(e)),
    }
}
```

Semantics (docs/dev/dsl/ir.md): a call binds its result on Ok and continues;
on Err it dispatches the `Err` arm (if present) and **stops the plan**. Error
codes are stable integers — never strings.

## Transcript testing (record/replay)

Host-side, effects run against **transcripts**: checked-in text fixtures of
request→response exchanges (`nexus-dsl-runtime`'s `svc::TranscriptHost`).

```text
# nx-transcript v1
call library.list() -> Ok(List[Str("Alpha"),Str("Beta")])
call db.put(Str("k"),Str("v")) -> Err(3)
query library(order=name,desc=false,limit=20,token="",eq=,low=,high=) -> Ok(next="",rows=List[])
```

The contract:

- replay matches invocations **in order and byte-exactly** (canonical value
  text codec, `svc::value_to_text`/`parse_value`);
- a divergence is a **replay miss**: the call returns the distinguished
  `ERR_TRANSCRIPT_MISS` code, the miss is recorded on the host, and
  `is_clean()` is false — a miss can never masquerade as a success;
- a malformed transcript fails at parse time (line number), not at replay;
- recording is explicit: `svc::Recorder` wraps a live host and emits the same
  format (recordings become checked-in fixtures via a dev flow, never
  silently).

`nx-dsl run <app> --transcript t.txt --dispatch LoadRequested` replays a
transcript headlessly and fails (exit 1) on any miss.

## The `query` effect step

Query-shaped data loading does not go through `svc.*` — it uses the dedicated
query step (the ONLY execution site for a declared `Query`, see
[db-queries.md](db-queries.md) for the declaration syntax and the engine):

```nx
@effect on Refresh {
    match LibraryItems(token: state.nextToken) {
        Ok(rows, next) => dispatch(Loaded(rows, next)),
        Err(e) => dispatch(LoadFailed(e)),
    }
}
```

- `Ok(rows, next)` binds the page rows and the opaque continuation token
  (`""` = exhausted) — store the token in state and re-dispatch to page;
- `Err(e)` binds a stable error code and stops the plan;
- the runtime resolves the declared spec + call-site params into a flattened
  `QueryCall` and hands it to the host (`EffectHost::query`): fixtures back it
  with the real `nexus-query` engine, the app-host speaks the queryd wire
  contract (`queryspec.capnp`), gated by `nexus.permission.QUERY`.

## Query posture

For query-shaped data loading, use this split:

- build the `Query` declaration as a pure top-level value,
- execute it only via the query step in effects,
- keep limits/order/page tokens explicit in state,
- and return stable error codes rather than stringly failures.

Good fit: picker/files/content-provider queries, history/bookmark/search-like
local indexes, feed/timeline cache views, connector-backed tables.

Bad fit: command actions, mutations/settings writes, send/launch/open side
effects, protocol/session control flows — those remain domain-specific
service calls even if they internally consult queryable storage.

## App-native services + app exports (planned, TASK-0081)

Two service classes extend the surface beyond the platform set — both keep
the exact `svc.*` mechanics above:

- **Companion services** (`native/` in the app folder): a normal Rust crate
  the tooling turns into the app's OWN process with its OWN manifest caps.
  The developer declares the surface once; the build generates the Rust
  server skeleton, the `svc.<app>.<method>()` checker signatures, and the
  manifest segment. Qt-like convenience, process-hard capability boundary.
  Companion crates may link only the curated SDK crate set (audio/video/gfx/
  text processing); device actuation stays behind platform services + caps.
- **App exports** (manifest v2.2 `exports`): an app exposes abilities under
  its OWN permission namespace (`app.<bundle>.<CAP>`); consumers declare the
  permission in `caps`. abilitymgr checks both sides fail-closed, launches
  the target if needed, mints the channel — then the apps talk DIRECTLY
  (no broker in the data path). The consumer sees generated
  `svc.app_<bundle>.<method>()` signatures.

## Changelog

- **v0.2b (2026-07-06, TASK-0078/0078B)** — generated signature table from
  `dsl_services.capnp` (NX0207/NX0208/NX0302), TranscriptHost record/replay +
  miss contract, the `query` effect step, queryd wire contract pointer.
