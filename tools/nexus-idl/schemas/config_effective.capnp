@0xf8d44d2f62c4e0a1;

struct EffectiveConfig {
  schemaVersion @0 :UInt16;
  dsoftbus @1 :Dsoftbus;
  metrics @2 :Metrics;
  tracing @3 :Tracing;
  securitySandbox @4 :SecuritySandbox;
  sched @5 :Sched;
  policy @6 :Policy;
}

struct Dsoftbus {
  transport @0 :Text;
  maxPeers @1 :UInt16;
}

struct Metrics {
  enabled @0 :Bool;
  flushIntervalMs @1 :UInt32;
}

struct Tracing {
  level @0 :Text;
  sampleRatePermille @1 :UInt16;
}

struct SecuritySandbox {
  defaultProfile @0 :Text;
  maxCaps @1 :UInt16;
}

struct Sched {
  defaultQos @0 :Text;
  runqueueSliceMs @1 :UInt16;
}

struct Policy {
  root @0 :Text;
}
