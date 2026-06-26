@0xf0f0c5c005500001;

# CONTEXT: TASK-0055 windowd surface IPC seed contract.
# OWNERS: @ui @runtime
# STATUS: DESCRIPTIVE ONLY — not code-generated, generates nothing.
# API_STABILITY: Internal v1b seed
# ADR: docs/adr/0038-display-wire-ssot-and-capnp-boundary.md
#
# The live windowd↔gpud wire SSOT is the Rust crate `nexus-display-proto`
# (opcodes/status/cursor magics + control-frame codecs) plus the
# `nexus_gfx::CommittedBuffer` payload codec. This schema is human documentation
# of the surface model and is NOT compiled into the build.

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

struct AcquireBackBufferRequest {
  surfaceId @0 :UInt64;
  frameIndex @1 :UInt64;
  width @2 :UInt32;
  height @3 :UInt32;
  strideBytes @4 :UInt32;
  format @5 :PixelFormat;
  vmoHandle @6 :UInt64;
}

struct PresentFrameRequest {
  surfaceId @0 :UInt64;
  frameIndex @1 :UInt64;
  damage @2 :List(DamageRect);
}

struct PresentFrameAck {
  fenceId @0 :UInt64;
  frameIndex @1 :UInt64;
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
  invalidFrameIndex @14;
  schedulerQueueFull @15;
}
