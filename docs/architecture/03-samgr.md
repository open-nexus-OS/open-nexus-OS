<!-- Copyright 2024 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Service Manager (samgr) Host Backend

The userspace service manager crate (`userspace-samgr`) provides a host-first
implementation of the service registry used by NEURON. The default
`backend-host` feature exposes an in-memory registry with the following API:

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
| `Unsupported`   | Returned by the placeholder `backend-os` backend.|

Generations are monotonically increasing `u64` values that allow callers to
reject stale handles after a restart. The host backend stores service records
inside a `parking_lot::Mutex<HashMap<_, _>>`, making it suitable for property
and unit tests without kernel dependencies.

The `backend-os` feature is a stub that always returns
`Error::Unsupported`. Future work will wire this variant into the actual
system call layer once the kernel exposes registry primitives.
