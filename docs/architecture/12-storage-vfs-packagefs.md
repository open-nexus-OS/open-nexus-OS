# Storage: `packagefsd` + `vfsd` — onboarding

The userspace “read-only bundle filesystem” is composed of two daemons:

- `packagefsd`: maintains a registry of bundle contents published by `bundlemgrd`
- `vfsd`: provides the client-facing VFS service and forwards requests to providers (currently `packagefs`)

Canonical sources:

- VFS overview: `docs/storage/vfs.md`
- Packaging and publication: `docs/packaging/nxb.md`
- Service architecture context: `docs/adr/0017-service-architecture.md`
- Testing guide + marker discipline: `docs/testing/index.md` and `scripts/qemu-test.sh`

## Responsibilities

### `packagefsd`

- Tracks installed bundles (published by `bundlemgrd` after successful install).
- Exposes read-only content under `/packages/<name>@<version>/...`.
- Maintains the active alias mapping used by `pkg:/<name>/...`.

### `vfsd`

- Exposes the Cap’n Proto VFS interface to clients.
- Holds a mount table and routes lookups to file system providers.
- Enforces read-only semantics (open/read/stat/close; reject writes).

## Client boundary

Clients should use the `nexus-vfs` crate.

- Host tests can use loopback wiring.
- OS builds route over kernel IPC.

## Proof and markers

Storage/VFS behavior is proven in two layers:

- Host integration tests (`tests/vfs_e2e`, related harnesses)
- QEMU smoke run marker contract (`scripts/qemu-test.sh`)

When you change path semantics, aliasing, or error behavior:

- update the owning task(s) stop conditions,
- update host E2E tests,
- and update the canonical QEMU harness expectations if and only if the real behavior changed.
