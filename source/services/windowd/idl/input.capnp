@0xf0f0c5c005500004;

# CONTEXT: TASK-0055 windowd input stub IPC seed contract.
# OWNERS: @ui @runtime
# STATUS: Stub only; real routing is TASK-0056B.
# API_STABILITY: Internal v1b seed

struct InputSubscribeRequest {
  surfaceId @0 :UInt64;
}

struct InputStubAck {
  status @0 :InputStubStatus;
}

enum InputStubStatus {
  unsupportedStub @0;
}
