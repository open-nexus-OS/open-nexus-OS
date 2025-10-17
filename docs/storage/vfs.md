# Userspace virtual file system

The userspace VFS is composed of two daemons:

* `packagefsd` maintains an in-memory registry of read-only bundle contents.
  Bundles are published by `bundlemgrd` after a successful install. Each bundle
  exposes a manifest, the executable payload, and optional assets below
  `assets/`.
* `vfsd` provides the client-facing Cap'n Proto service. It keeps a mount table
  and forwards lookups to individual file system providers (currently only
  `packagefs`).

`bundlemgrd` publishes bundles under `/packages/<name>@<version>/...` and marks
that version as the active alias for `pkg:/<name>/...`. All paths are
read-only—open, read, stat, and close are supported while write operations are
rejected. Invalid paths, missing entries, or reuse of closed file handles are
reported via the `ok=false` field in the Cap'n Proto responses.

Clients can depend on the `nexus-vfs` crate to talk to the service. On host
builds `VfsClient::from_loopback` wires tests directly to a loopback server,
while OS builds use the kernel IPC channel.
