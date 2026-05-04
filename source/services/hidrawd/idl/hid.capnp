@0xf0f0c5c005530001;

# CONTEXT: TASK-0253 `hidrawd` subscribe contract seed for deterministic keyboard/mouse ingest.
# OWNERS: @ui @runtime
# STATUS: Experimental
# API_STABILITY: Unstable

struct SubscribeRequest {
}

struct DeviceInfo {
  id @0 :UInt64;
  kind @1 :DeviceKind;
}

enum DeviceKind {
  keyboard @0;
  mouse @1;
}

struct HidEvent {
  timestampNs @0 :UInt64;
  kind @1 :HidEventKind;
  code @2 :UInt32;
  value @3 :Int32;
}

enum HidEventKind {
  key @0;
  button @1;
  relative @2;
}

struct HidBatch {
  device @0 :DeviceInfo;
  events @1 :List(HidEvent);
}

struct SubscribeResponse {
  devices @0 :List(DeviceInfo);
  stream @1 :List(HidBatch);
}
