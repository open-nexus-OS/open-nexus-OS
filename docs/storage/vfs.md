# Userspace virtual file system

The userspace VFS is composed of two daemons:

* `packagefsd` serves the read-only `pkg:/` namespace out of the VERIFIED SYSTEM
  VOLUME (TASK-0321 P5): the bundle index comes from `bundlemgrd` (`GET_INDEX`,
  the NXSV-bound bytes it verified) and entry bytes are never held — they are
  fetched on demand, or, above the inline tier, not fetched at all but written
  straight into the caller's VMO by `bundlemgrd` (see "How bytes move" below).
  Each bundle exposes a manifest, the executable payload, and optional assets
  below `assets/`.
* `vfsd` provides the client-facing Cap'n Proto service. It keeps a mount table
  and forwards lookups to individual file system providers (currently only
  `packagefs`).

`bundlemgrd` publishes bundles under `/packages/<name>@<version>/...` and marks
that version as the active alias for `pkg:/<name>/...`. All paths are
read-only—open, read, stat, and close are supported while write operations are
rejected. Invalid paths, missing entries, or reuse of closed file handles are
reported via the `ok=false` field in the Cap'n Proto responses.

## How bytes move

There are two tiers, and which one applies is decided by the entry's size, not by
the caller (RFC-0072, RFC-0097):

| entry size | path | who writes the bytes |
|---|---|---|
| ≤ `INLINE_IO_MAX` (4096 B) | inline read, one bounded reply frame | packagefsd, from its own small VMO |
| > `INLINE_IO_MAX` | `OP_READ_VMO`: the caller's VMO is cap-moved vfsd → packagefsd → bundlemgrd | **bundlemgrd only**, after the entry hashes to its index digest |

An inline read above the cap is `E2BIG`, never a truncation. A VMO too small for
the entry is refused with `TooBig` in the header before a single byte is written
— packagefsd checks it, being the hop that knows both the entry's size and the
VMO's length.

Only the hop that verified the bytes may write a success header; every other hop
may write an ERROR header and nothing else. That is what makes `pkg:/` reads
zero-copy in the honest sense: vfsd and packagefsd relay a capability, not data.

Until TASK-0033 this was not true. `pkg:/` entry bytes rode inline in
packagefsd's reply, so they were bounded by the IPC frame cap — measured on the
shipped volume, **22 of its 115 entries were unreadable, and asking for one ended
packagefsd** (`E2BIG` on the reply, or `alloc_error` on a heap that never frees).
Every proof was green because the only `pkg:/` file any lane read was
`pkg:/system/build.prop`, 19 bytes.

Clients can depend on the `nexus-vfs` crate to talk to the service. On host
builds `VfsClient::from_loopback` wires tests directly to a loopback server,
while OS builds use the kernel IPC channel.
