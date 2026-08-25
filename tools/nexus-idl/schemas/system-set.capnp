@0xd8b8c2a7f2cc1a50;
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

# System-Set index schema (system.nxsindex)
#
# Canonical source of truth for .nxs metadata and bundle digests.
# See RFC-0012 for the contract and verification rules.
#
# VERSIONING:
#   - schemaVersion field tracks schema evolution
#   - v1.0: Core fields + bundle digests
#   - v1.1+: Additive fields only; gate by schemaVersion
#
# USAGE:
#   - Pack: nxs-pack → system.nxsindex (Cap'n Proto binary)
#   - Verify: updated reads system.nxsindex + system.sig.ed25519

struct SystemSetIndex {
  schemaVersion @0 :UInt8 = 1;
  systemVersion @1 :Text;
  publisher @2 :Data;          # 32 bytes
  timestampUnixMs @3 :UInt64;  # metadata; not used in markers
  bundles @4 :List(BundleEntry);
}

struct BundleEntry {
  name @0 :Text;
  version @1 :Text;            # SemVer string
  manifestSha256 @2 :Data;     # 32 bytes
  payloadSha256 @3 :Data;      # 32 bytes
  payloadSize @4 :UInt64;
}

# .nxs v2 — signed COMPONENT manifest (RFC-0089 §3, `manifest.nxo`).
#
# The v2 container evolves the update unit from a bundles-only set to typed
# components: v1 ships exactly ONE kind (`boot-image` = 1); `bundle` (2),
# `boot-image-delta` (3), `bundle-delta` (4) and `rotation-record` (5) are
# RESERVED — an unknown kind is a deterministic reject, never a skip.
# Verification order is normative: manifest signature against the DEVICE
# anchor (the publisherKeyId is a lookup hint, never a trust input) →
# rollbackIndex >= persisted floor → streamed per-component sha256 →
# kind-specific checks. Produced by `nx image ota`; consumed by `updated`.

struct ComponentManifest {
  schemaVersion @0 :UInt8 = 2;
  publisherKeyId @1 :Data;     # 8 bytes — anchor lookup hint
  buildId @2 :Text;            # deterministic build identifier
  rollbackIndex @3 :UInt32;    # monotonic anti-downgrade index (§10)
  components @4 :List(Component);
}

struct Component {
  kind @0 :UInt8;              # 1 = boot-image (v1); 2..5 reserved
  name @1 :Text;
  size @2 :UInt64;
  sha256 @3 :Data;             # 32 bytes over the payload entry bytes
  payloadPath @4 :Text;        # tar entry path
  kindData @5 :Data;           # kind-specific, bounded; empty for boot-image
}
