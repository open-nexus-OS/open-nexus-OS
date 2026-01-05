<!-- Copyright 2024 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Service Manager (samgr) Host Backend

**Scope:** this page documents the **host-first** `userspace/samgr` library.  
For the OS service manager daemon, see `docs/architecture/14-samgrd-service-manager.md`.

The userspace service manager crate (`userspace/samgr`) provides a **host-first**
service registry used by tests and host harnesses. When compiled with
`RUSTFLAGS='--cfg nexus_env="host"'`, it exposes an in-memory registry with the following API:

- `register(name, endpoint) -> ServiceHandle` registers a service if the name
  is unused.
- `resolve(name) -> ServiceHandle` returns the latest endpoint and generation
  number for a service.
- `heartbeat(handle)` refreshes liveness information for the supplied
  generation.
- `restart(name, endpoint) -> ServiceHandle` increments the generation and
  replaces the endpoint.

All operations return a `Result<T, Error>` where `Error` covers the following
cases:

| Variant         | Description                                      |
|-----------------|--------------------------------------------------|
| `Duplicate`     | A service with the given name already exists.    |
| `NotFound`      | The requested service does not exist.            |
| `StaleHandle`   | The provided handle references an old generation.|
| `Unsupported`   | Returned by the `nexus_env="os"` stub backend.   |

Generations are monotonically increasing `u64` values that allow callers to
reject stale handles after a restart. The host backend stores service records
inside a `parking_lot::Mutex<HashMap<_, _>>`, making it suitable for property
and unit tests without kernel dependencies.

## Relationship to the OS

The `samgr` crate is intentionally **not** the OS service manager implementation.
The OS uses the `samgrd` daemon (see `docs/adr/0017-service-architecture.md`) and routes requests via IPC (`userspace/nexus-ipc`) and IDL bindings (`userspace/nexus-idl-runtime`).

The `nexus_env="os"` backend in `userspace/samgr` remains a stub that returns `Error::Unsupported` to prevent accidental “in-proc registry” drift in OS builds.
