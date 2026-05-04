@0xf0f0c5c005530002;

# CONTEXT: TASK-0253 `touchd` subscribe contract seed for normalized touch events.
# OWNERS: @ui @runtime
# STATUS: Experimental
# API_STABILITY: Unstable

struct SubscribeRequest {
}

struct DeviceInfo {
  id @0 :UInt64;
  syntheticMode @1 :Bool;
}

enum TouchPhase {
  down @0;
  move @1;
  up @2;
}

struct TouchEvent {
  timestampNs @0 :UInt64;
  x @1 :UInt32;
  y @2 :UInt32;
  phase @3 :TouchPhase;
}

struct SubscribeResponse {
  devices @0 :List(DeviceInfo);
  stream @1 :List(TouchEvent);
}
