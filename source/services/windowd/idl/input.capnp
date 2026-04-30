@0xf0f0c5c005500004;

# CONTEXT: TASK-0055 windowd input stub IPC seed contract; TASK-0056 adds v2a routing.
# OWNERS: @ui @runtime
# STATUS: Internal v2a seed
# API_STABILITY: Internal v2a seed

struct InputSubscribeRequest {
  surfaceId @0 :UInt64;
}

struct InputStubAck {
  status @0 :InputStubStatus;
}

enum InputStubStatus {
  unsupportedStub @0;
}

struct PointerEvent {
  x @0 :Int32;
  y @1 :Int32;
  kind @2 :PointerKind;
}

struct KeyboardEvent {
  keyCode @0 :UInt32;
}

struct InputDelivery {
  inputSeq @0 :UInt64;
  surfaceId @1 :UInt64;
  kind @2 :InputDeliveryKind;
}

enum PointerKind {
  down @0;
}

enum InputDeliveryKind {
  pointerDown @0;
  keyboard @1;
}

enum InputError {
  ok @0;
  staleSurfaceId @1;
  unauthorized @2;
  noFocusedSurface @3;
  inputEventQueueFull @4;
}
