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
#   - v1.2: Add sbomDigest + reproDigest (TASK-0029)
#   - v2.0: bundleType + dependencies + providedServices + resources
#   - v2.2: exports (TASK-0081 — app-owned permission namespaces for
#     app-to-app abilities; mediated-then-direct via abilitymgr).
#   - v2.1: payloadKind (TASK-0080D — DSL apps ship payload.nxir; execd
#           dispatches uiProgram payloads to the app-host runtime)
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

  # v1.2 additions (TASK-0029)
  # SHA-256 digest of meta/sbom.json (32 bytes)
  sbomDigest @10 :Data;

  # SHA-256 digest of meta/repro.env.json (32 bytes)
  reproDigest @11 :Data;

  # v2.0 additions (TASK-0057 Phase 4)
  # Bundle type: what kind of artifact this is.
  bundleType @12 :BundleType = app;
  
  struct Dependency {
    name @0 :Text;
    versionConstraint @1 :Text;  # e.g. "^1.2.0"
  }
  # Services/libraries this bundle depends on.
  dependencies @13 :List(Dependency);
  
  # Service names this bundle provides (registered with samgrd on install).
  providedServices @14 :List(Text);
  
  struct Resource {
    path @0 :Text;      # Relative path within bundle (e.g. "icons/app.svg")
    kind @1 :ResourceKind = icon;  # icon | cursor | font | wallpaper | sound | data
  }
  # Resources bundled with this artifact.
  resources @15 :List(Resource);

  # v2.1 addition (TASK-0080D)
  # What the payload IS: a native ELF (spawned directly) or a compiled DSL
  # UI program (`payload.nxir`, loaded by the app-host runtime process).
  # Append-only; readers of older manifests see the `elf` default.
  payloadKind @16 :PayloadKind = elf;

  # v2.2 addition (TASK-0081 decision C2): app-to-app exports. An app
  # exposes abilities under its OWN permission namespace
  # (`app.<bundle>.<CAP>`); consumers declare that permission in `caps`.
  # abilitymgr checks BOTH sides fail-closed, launches the exporter if
  # needed, mints the endpoint pair — then the apps talk DIRECTLY
  # (mediated-then-direct; no broker in the data path). Append-only.
  exports @17 :List(ExportDecl);
}

# One exported ability + the app-owned permission gating its consumers.
struct ExportDecl {
  ability @0 :Text;      # e.g. "chat.Send"
  permission @1 :Text;   # e.g. "app.chat.SEND" (MUST be app.<bundle>.<CAP>)
}

enum PayloadKind {
  elf @0;
  uiProgram @1;
}

enum BundleType {
  app @0;
  service @1;
  library @2;
  driver @3;
  framework @4;
}

enum ResourceKind {
  icon @0;
  cursor @1;
  font @2;
  wallpaper @3;
  sound @4;
  data @5;
}

# Validation rules (enforced by parser):
#
# 1. name: Non-empty, trimmed, valid identifier
# 2. semver: Valid SemVer (parseable by semver crate)
# 3. abilities: At least one entry for app/service; empty OK for library
# 4. capabilities: Non-empty strings (empty list OK)
# 5. minSdk: Valid SemVer
# 6. publisher: Exactly 16 bytes
# 7. signature: Exactly 64 bytes
# 8. payloadDigest: Exactly 32 bytes (if present)
# 9. payloadSize: > 0 (if present)
# 10. sbomDigest: Exactly 32 bytes (if present)
# 11. reproDigest: Exactly 32 bytes (if present)
# 12. dependencies: Each name non-empty, versionConstraint valid SemVer range
# 13. providedServices: Non-empty strings (empty list OK)
# 14. resources: Each path non-empty, kind valid enum value
#
# Parser MUST reject manifests violating these rules.
