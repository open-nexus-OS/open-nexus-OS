@0xdeadc0de12344321;

struct PublishBundle {
  name @0 :Text;
  version @1 :Text;
  rootVmo @2 :UInt32;
  entries @3 :List(FileEntry);
}
struct PublishResponse { ok @0 :Bool; }

struct ResolvePath { rel @0 :Text; }
struct ResolveResponse { ok @0 :Bool; size @1 :UInt64; kind @2 :UInt16; bytes @3 :Data; }

# Directory listing (RFC-0072 Phase 1 provider hook). `rel = "."` lists the
# bundle roots; `rel = "<bundle>[/<sub>]"` lists direct children. `err` codes
# are the stable storage error SSOT (nexus-vfs-types, append-only).
struct PkgDirEntry { name @0 :Text; kind @1 :UInt16; size @2 :UInt64; }
struct ListPath { rel @0 :Text; cursor @1 :UInt32; limit @2 :UInt16; }
struct ListResponse { ok @0 :Bool; err @1 :UInt16; entries @2 :List(PkgDirEntry); nextCursor @3 :UInt32; eof @4 :Bool; }

struct FileEntry {
  path @0 :Text;
  kind @1 :UInt16;
  bytes @2 :Data;
}
