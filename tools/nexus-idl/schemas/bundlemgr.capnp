@0xb1b1b1b1b1b1b1b1;
enum InstallError {
  none @0;
  eacces @1;
  einval @2;
  ebusy @3;
  enoent @4;
  invalidSig @5;
  unsigned @6;
}
struct InstallRequest { name @0 :Text; bytesLen @1 :UInt32; vmoHandle @2 :UInt32; }
struct InstallResponse { ok @0 :Bool; err @1 :InstallError; }
struct QueryRequest { name @0 :Text; }
struct QueryResponse { installed @0 :Bool; version @1 :Text; requiredCaps @2 :List(Text); }
struct GetPayloadRequest { name @0 :Text; }
struct GetPayloadResponse { ok @0 :Bool; bytes @1 :Data; }
# RFC-0065: app registry enumeration — the "which apps exist" listing the
# launcher/SystemUI query instead of hardcoding an app list.
struct AppRecord {
  id @0 :Text;
  displayName @1 :Text;
  launchAbility @2 :Text;
  requiredCaps @3 :List(Text);
}
struct EnumerateRequest {}
struct EnumerateResponse { apps @0 :List(AppRecord); }
