@0xa1a1a1a1a1a1a1a1;
struct RegisterRequest { name @0 :Text; endpoint @1 :UInt32; }
struct RegisterResponse { ok @0 :Bool; }
struct ResolveRequest { name @0 :Text; }
struct ResolveResponse { endpoint @0 :UInt32; found @1 :Bool; }
struct Heartbeat { endpoint @0 :UInt32; }
