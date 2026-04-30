@0xf0f0c5c005500003;

# CONTEXT: TASK-0055 windowd headless vsync/present IPC seed contract.
# OWNERS: @ui @runtime
# STATUS: Done
# API_STABILITY: Internal v1b seed

struct SubscribeVsyncRequest {
  lastSeenPresentSeq @0 :UInt64;
}

struct PresentAck {
  presentSeq @0 :UInt64;
  damageRectCount @1 :UInt16;
  hz @2 :UInt16;
}

struct ScheduledPresentAck {
  presentSeq @0 :UInt64;
  damageRectCount @1 :UInt16;
  framesCoalesced @2 :UInt16;
  fencesSignaled @3 :UInt16;
  latencyMs @4 :UInt32;
}

struct PresentFenceStatus {
  fenceId @0 :UInt64;
  frameIndex @1 :UInt64;
  signaled @2 :Bool;
  coalesced @3 :Bool;
  presentSeq @4 :UInt64;
}

enum PresentError {
  ok @0;
  noCommittedScene @1;
  noDamage @2;
  staleSequence @3;
  unauthorized @4;
  invalidFrameIndex @5;
  schedulerQueueFull @6;
  fenceNotReady @7;
}
