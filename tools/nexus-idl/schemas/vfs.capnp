@0xdeadc0dedeadc0de;

struct OpenRequest { path @0 :Text; }
struct OpenResponse { ok @0 :Bool; fh @1 :UInt32; size @2 :UInt64; kind @3 :UInt16; }

struct ReadRequest { fh @0 :UInt32; off @1 :UInt64; len @2 :UInt32; }
struct ReadResponse { ok @0 :Bool; bytes @1 :Data; }

struct CloseRequest { fh @0 :UInt32; }
struct CloseResponse { ok @0 :Bool; }

struct StatRequest { path @0 :Text; }
struct StatResponse { ok @0 :Bool; size @1 :UInt64; kind @2 :UInt16; }

struct MountRequest { mountPoint @0 :Text; fsId @1 :Text; }
struct MountResponse { ok @0 :Bool; }
