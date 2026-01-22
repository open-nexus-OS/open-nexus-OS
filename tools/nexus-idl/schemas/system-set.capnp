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
