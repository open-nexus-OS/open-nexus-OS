@0xd9a4f1c2b8e30765;

# CONTEXT: gpud GPU driver IPC contract — windowd→gpud command protocol.
# OWNERS: @ui @runtime
# STATUS: DESCRIPTIVE ONLY — not code-generated, generates nothing.
# API_STABILITY: Internal v1
# RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md
# ADR: docs/adr/0038-display-wire-ssot-and-capnp-boundary.md
#
# SSOT (the actual source of truth, Gate 2): the Rust crate
# `source/libs/nexus-display-proto` owns the opcodes / status codes / cursor reply
# magics and the small control-frame codecs; the per-frame payload is the
# `nexus_gfx::CommittedBuffer` codec. windowd AND gpud both import the crate, so
# the wire is defined exactly once. This `.capnp` is human documentation kept in
# step with that crate — it is NOT compiled and does NOT govern the wire.
#
# Architecture: windowd (sole display owner) → gpud (sole GPU consumer).
# Zero-copy path: windowd creates the framebuffer VMO; kernel transfers cap
# ownership to gpud (ATTACH_BACKING). All pixel data lives in the VMO —
# no per-pixel IPC, no memcpy across process boundaries.
#
# Wire format: each IPC message has a 1-byte opcode as defined by OpCode.
# Responses are fixed-size: [status:u8] or [status:u8, handoff_id:u32le].
# The CommittedBuffer payload is serialized by nexus-gfx (not Cap'n Proto)
# and carried as opaque bytes after the opcode.
#
# This schema documents intent for chain-test contracts. The canonical wire is
# the `nexus-display-proto` crate (constants + control-frame codecs) + the
# `nexus_gfx::CommittedBuffer` payload codec — see the ADR above.

# ── Opcodes ──────────────────────────────────────────────────────────────────

enum OpCode {
  # Submit a serialized CommittedBuffer (animation frame, no scanout update).
  submitAnimationFrame @0;   # wire: 0x01

  # Move the hardware cursor to (x, y) — deprecated, use BlendCursor in CB.
  moveCursor @1;             # wire: 0x02

  # Attach the shared framebuffer VMO. Carries a capability in the IPC cap slot.
  # Payload: [opcode:u8, handoff_id:u32le]
  setFramebufferVmo @2;      # wire: 0x03

  # Present with damage. Payload: [opcode:u8, CommittedBuffer...] (preferred)
  # or legacy [opcode:u8, x:u32le, y:u32le, w:u32le, h:u32le, handoff_id:u32le].
  presentDamage @3;          # wire: 0x04

  # Upload cursor bitmap for software BlendCursor compositing.
  # Payload: [opcode:u8, w:u32le, h:u32le, bgra:w×h×4 bytes]
  uploadCursor @4;           # wire: 0x05
}

# ── Status codes (response byte 0) ───────────────────────────────────────────

enum StatusCode {
  ok @0;            # wire: 0x00
  malformed @1;     # wire: 0x01 — payload too short or unknown opcode
  deviceError @2;   # wire: 0x02 — virtio-gpu rejected the command
}

# ── Request structs (documentation; not yet code-generated) ──────────────────

struct SetFramebufferVmoRequest {
  # Carries the VMO capability in the IPC cap slot (not in this struct).
  # The kernel transfers VMO ownership from windowd to gpud atomically.
  handoffId @0 :UInt32;
  # VMO geometry (must match windowd constants).
  width @1 :UInt32;         # DISPLAY_WIDTH = 1280
  resourceHeight @2 :UInt32; # RESOURCE_HEIGHT = 3200 (4-plane layout)
}

struct SetFramebufferVmoResponse {
  status @0 :StatusCode;
  handoffId @1 :UInt32;
}

struct PresentDamageRequest {
  # Preferred path: CommittedBuffer follows immediately after opcode byte.
  # gpud deserializes the CB and executes: BlitSurface regions → BlurBackdrop
  # panels → FillSdfRoundedRect glass → BlendCursor overlay → scanout flush.
  # The damage rect for TRANSFER_TO_HOST_2D is derived from all CB commands.
  handoffId @0 :UInt32;

  # Legacy path (17-byte fixed frame): used when CB deserialization fails and
  # frame is exactly [opcode + 16 bytes]. Deprecated; prefer CB path.
  legacyDamageX @1 :UInt32;
  legacyDamageY @2 :UInt32;
  legacyDamageW @3 :UInt32;
  legacyDamageH @4 :UInt32;
}

struct PresentDamageResponse {
  status @0 :StatusCode;
  handoffId @1 :UInt32;
}

struct UploadCursorRequest {
  width @0 :UInt32;
  height @1 :UInt32;
  # BGRA pixel data follows: width × height × 4 bytes after the fixed header.
  # Stored as a software sprite for BlendCursor (no hardware cursor resource).
}

struct MoveCursorRequest {
  # Deprecated: cursor is rendered via BlendCursor in the frame CommandBuffer.
  # Kept for backward compatibility only. gpud ignores x/y if BlendCursor
  # is present in the submitted CB.
  x @0 :Int32;
  y @1 :Int32;
}

# ── CommandBuffer wire vocabulary (nexus-gfx, not Cap'n Proto) ───────────────
#
# Commands serialized inside PresentDamage / SubmitAnimationFrame payloads.
# Each command: [tag:u8, fields...] — 25 bytes for most, 17 for BlendCursor.
#
#   BlitSurface:        src_x, src_y, dst_x, dst_y, width, height  (6×u32)
#   FillSdfRoundedRect: x, y, width, height, radius, color_rgba    (5×u32+u32)
#   BlurBackdrop:       x, y, width, height, radius, saturation_pct(5×u32+u32)
#   BlendCursor:        x, y, width, height                        (4×u32)
#   SetFragmentBytes:   offset, data_len, data...                  (variable)
#   DrawTiles:          tile_count, tiles...                        (variable)
#
# Frame layout (retained-surface model, RFC-0059 §GPU-first pipeline):
#   1. BlitSurface: damage rects, Plane 1 (retained) → Plane 2 (display)
#   2. BlitSurface + BlurBackdrop + FillSdfRoundedRect: glass button overlay
#   3. [when sidebar open] BlitSurface + BlurBackdrop + FillSdfRoundedRect: sidebar
#   4. BlendCursor: software cursor composited last
#
# Plane layout in the 16MB VMO (1280×3200, 4-plane):
#   Plane 0: rows    0..799  — wallpaper source   (offset 0x000000)
#   Plane 1: rows  800..1599 — retained scene     (offset 0x3E8000)
#   Plane 2: rows 1600..2399 — display/scanout    (offset 0x7D0000)
#   Plane 3: rows 2400..3199 — slot B (reserved)  (offset 0xBB8000)

# ── Timer-driven animation IPC (RFC-0062 Phase D.3, planned) ─────────────────
#
# Current path (Phase D.1 workaround): windowd uses Wait::Timeout(8.3ms)
# as an IPC deadline. On timeout, the kernel returns IpcError::TimedOut,
# which windowd interprets as an animation tick. Drawback: drift accumulates.
#
# Planned path (RFC-0062 Phase D.2+): kernel Timer capability fires OP_TIMER_FIRED
# into windowd's recv queue at each deadline. windowd calls tick(now_ns) and
# submits the animated frame via PresentDamage. gpud sends PresentDamageResponse
# on completion; windowd uses the ack to rearm the timer for the next deadline.
#
# The vsync feedback schema is in windowd/idl/vsync.capnp (PresentAck /
# ScheduledPresentAck). gpud will eventually emit vsync pulses over that channel
# so windowd can pace frame production to display refresh rate.
