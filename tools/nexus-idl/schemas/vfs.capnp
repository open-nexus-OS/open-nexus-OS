@0xdeadc0dedeadc0de;

# Error codes (`err` fields) are the stable storage error SSOT (RFC-0072):
# 0 = OK; nonzero values are defined in `nexus-vfs-types` (append-only).
# `err` fields are additive; `ok` MUST equal `err == 0`.

struct OpenRequest { path @0 :Text; }
struct OpenResponse { ok @0 :Bool; fh @1 :UInt32; size @2 :UInt64; kind @3 :UInt16; err @4 :UInt16; }

struct ReadRequest { fh @0 :UInt32; off @1 :UInt64; len @2 :UInt32; }
struct ReadResponse { ok @0 :Bool; bytes @1 :Data; err @2 :UInt16; }

struct CloseRequest { fh @0 :UInt32; }
struct CloseResponse { ok @0 :Bool; err @1 :UInt16; }

struct StatRequest { path @0 :Text; }
struct StatResponse { ok @0 :Bool; size @1 :UInt64; kind @2 :UInt16; err @3 :UInt16; }

struct MountRequest { mountPoint @0 :Text; fsId @1 :Text; }
struct MountResponse { ok @0 :Bool; err @1 :UInt16; }

# ReadDir (RFC-0072 Phase 1): bounded pagination, canonical provider order.
struct DirEntry { name @0 :Text; kind @1 :UInt16; size @2 :UInt64; }
struct ReadDirRequest { path @0 :Text; cursor @1 :UInt32; limit @2 :UInt16; }
struct ReadDirResponse { ok @0 :Bool; err @1 :UInt16; entries @2 :List(DirEntry); nextCursor @3 :UInt32; eof @4 :Bool; }
