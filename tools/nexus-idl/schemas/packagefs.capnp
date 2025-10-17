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

struct FileEntry {
  path @0 :Text;
  kind @1 :UInt16;
  bytes @2 :Data;
}
