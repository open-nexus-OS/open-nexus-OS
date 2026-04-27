@0xf0f0c5c005500001;

# CONTEXT: TASK-0055 windowd surface IPC seed contract.
# OWNERS: @ui @runtime
# STATUS: Done
# API_STABILITY: Internal v1b seed

struct SurfaceCreateRequest {
  width @0 :UInt32;
  height @1 :UInt32;
  strideBytes @2 :UInt32;
  format @3 :PixelFormat;
  vmoHandle @4 :UInt64;
}

struct QueueBufferRequest {
  surfaceId @0 :UInt64;
  commitSeq @1 :UInt64;
  width @2 :UInt32;
  height @3 :UInt32;
  strideBytes @4 :UInt32;
  format @5 :PixelFormat;
  vmoHandle @6 :UInt64;
  damage @7 :List(DamageRect);
}

struct DamageRect {
  x @0 :UInt32;
  y @1 :UInt32;
  width @2 :UInt32;
  height @3 :UInt32;
}

enum PixelFormat {
  bgra8888 @0;
  unsupported @1;
}

enum SurfaceError {
  ok @0;
  invalidDimensions @1;
  invalidStride @2;
  unsupportedFormat @3;
  missingVmoHandle @4;
  forgedVmoHandle @5;
  wrongVmoRights @6;
  nonSurfaceBuffer @7;
  surfaceTooLarge @8;
  staleSurfaceId @9;
  unauthorized @10;
  invalidDamage @11;
  tooManySurfaces @12;
  tooManyDamageRects @13;
}
