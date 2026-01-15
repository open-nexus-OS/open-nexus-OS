@0xf8a3b2c1d4e5f6a7;
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

# Bundle Manifest Schema (manifest.nxb)
#
# This is the canonical, deterministic manifest format for .nxb bundles.
# See ADR-0020 for rationale and migration strategy.
#
# VERSIONING:
#   - schemaVersion field tracks schema evolution
#   - v1.0: Core fields (name, version, abilities, caps, publisher, signature)
#   - v1.1: Add payloadDigest + payloadSize (TASK-0034)
#   - v2.0+: Future extensions (dependencies, permissions, etc.)
#
# USAGE:
#   - Input: manifest.toml (human-editable)
#   - Compile: nxb-pack → manifest.nxb (Cap'n Proto binary)
#   - Parse: bundlemgr (host) + bundlemgrd (OS)
#
# DETERMINISM:
#   - Cap'n Proto canonical encoding ensures same data → same binary
#   - No whitespace/ordering ambiguity (unlike JSON/TOML)
#   - Directly signable (no "canonical JSON" hacks)

struct BundleManifest {
  # Schema version (for evolution)
  # v1.0 = 1, v1.1 = 1 (backward compatible), v2.0 = 2, etc.
  schemaVersion @0 :UInt8 = 1;

  # Core fields (v1.0)
  # Bundle identifier (unique, non-empty)
  name @1 :Text;

  # SemVer version string (e.g. "1.2.3")
  semver @2 :Text;

  # Declared abilities provided by this bundle
  # (e.g. ["ohos.ability.MainAbility", "ohos.ability.BackgroundService"])
  abilities @3 :List(Text);

  # Capability requirements requested by this bundle
  # (e.g. ["ohos.permission.INTERNET", "ohos.permission.CAMERA"])
  capabilities @4 :List(Text);

  # Minimum SDK version compatible with this bundle (SemVer)
  minSdk @5 :Text;

  # Publisher identifier (16 bytes, hex-decoded).
  #
  # Canonical representation in TOML/UI is 32 lowercase hex chars (16 bytes),
  # matching `keystore::device_id()` (SHA-256(pubkey) truncated to 16 bytes).
  publisher @6 :Data;

  # Detached Ed25519 signature covering the bundle payload
  # (64 bytes: R || S)
  signature @7 :Data;

  # v1.1 additions (TASK-0034)
  # SHA-256 digest of payload.elf (32 bytes)
  # Used for integrity verification on install
  payloadDigest @8 :Data;

  # Size of payload.elf in bytes
  # Used for download progress + storage checks
  payloadSize @9 :UInt64;

  # Future extensions (v2.0+)
  # Example placeholders (not implemented yet):
  #
  # struct Dependency {
  #   name @0 :Text;
  #   versionConstraint @1 :Text;  # e.g. "^1.2.0"
  # }
  #
  # struct Permission {
  #   name @0 :Text;
  #   reason @1 :Text;  # User-facing explanation
  # }
  #
  # dependencies @10 :List(Dependency);
  # permissions @11 :List(Permission);
  # icon @12 :Data;  # PNG/JPEG bytes
  # metadata @13 :Map(Text, Text);  # Key-value pairs
}

# Validation rules (enforced by parser):
#
# 1. name: Non-empty, trimmed, valid identifier
# 2. semver: Valid SemVer (parseable by semver crate)
# 3. abilities: At least one entry, non-empty strings
# 4. capabilities: Non-empty strings (empty list OK)
# 5. minSdk: Valid SemVer
# 6. publisher: Exactly 16 bytes
# 7. signature: Exactly 64 bytes
# 8. payloadDigest: Exactly 32 bytes (if present)
# 9. payloadSize: > 0 (if present)
#
# Parser MUST reject manifests violating these rules.
