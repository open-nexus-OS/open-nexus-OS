@0xf0f0c5c005500002;

# CONTEXT: TASK-0055 windowd layer/tree IPC seed contract.
# OWNERS: @ui @runtime
# STATUS: Done
# API_STABILITY: Internal v1b seed

struct SceneCommitRequest {
  commitSeq @0 :UInt64;
  layers @1 :List(LayerEntry);
}

struct LayerEntry {
  surfaceId @0 :UInt64;
  x @1 :Int32;
  y @2 :Int32;
  z @3 :Int16;
}

struct SceneCommitAck {
  acceptedSeq @0 :UInt64;
  error @1 :LayerError;
}

enum LayerError {
  ok @0;
  staleCommitSequence @1;
  staleSurfaceId @2;
  unauthorized @3;
  tooManyLayers @4;
  invalidLayerTree @5;
}
