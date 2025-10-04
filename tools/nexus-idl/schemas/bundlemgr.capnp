@0xb1b1b1b1b1b1b1b1;
enum InstallError { none @0; eacces @1; einval @2; ebusy @3; enoent @4; }
struct InstallRequest { name @0 :Text; bytesLen @1 :UInt32; vmoHandle @2 :UInt32; }
struct InstallResponse { ok @0 :Bool; err @1 :InstallError; }
struct QueryRequest { name @0 :Text; }
struct QueryResponse { installed @0 :Bool; version @1 :Text; }
