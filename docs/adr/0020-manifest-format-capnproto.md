<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0020: Bundle Manifest Format (manifest.nxb with Cap'n Proto)

**Status**: Accepted  
**Date**: 2026-01-15  
**Owners**: @runtime  
**Supersedes**: None (resolves format drift)

---

## Context

The repository had **three conflicting manifest formats** (historical drift):

1. **`docs/packaging/nxb.md`**: Documents `manifest.nxb` (binary, not implemented)
2. **`tools/nxb-pack`**: Generated `manifest.json` (JSON) *(fixed as part of this unification)*
3. **`bundlemgr` parser**: Parses TOML (`Manifest::parse_str`)

This drift creates:

- **Signature ambiguity**: Which format is signed?
- **Determinism issues**: JSON/TOML have whitespace/ordering variations
- **Tooling fragmentation**: Different formats in different layers

### Requirements

1. **Deterministic**: Same manifest data → same binary output (for signing)
2. **Fast parsing**: Especially in OS mode (no_std, bounded memory)
3. **Versionable**: Schema can evolve (v1 → v1.1 → v2)
4. **Signable**: Binary format is directly signed (no "canonical JSON" tricks)
5. **Host + OS support**: Works in std and no_std environments
6. **Tooling-friendly**: Human-editable source format (TOML) → compiled binary

---

## Decision

### **Single Source of Truth: `manifest.nxb` (Cap'n Proto binary)**

#### Format Hierarchy

```text

manifest.toml (human-editable, tooling input)
    ↓ nxb-pack compile
manifest.nxb (binary, Cap'n Proto, signable)
    ↓ bundlemgr/bundlemgrd parse
Manifest struct (in-memory)

```text

#### Why Cap'n Proto?

| Criterion | Cap'n Proto | JSON | TOML | Binary XML (Android) | Custom TLV |
|-----------|-------------|------|------|----------------------|------------|
| **Deterministic** | ✅ Canonical encoding | ❌ Whitespace/order | ❌ Whitespace/order | ✅ | ✅ |
| **Fast parsing** | ✅ Zero-copy | ❌ Tokenization | ❌ Tokenization | ✅ | ✅ |
| **Versionable** | ✅ Schema evolution | ⚠️ Manual | ⚠️ Manual | ⚠️ Manual | ❌ |
| **no_std support** | ✅ capnp-rust | ❌ serde needs alloc | ❌ toml needs alloc | ❌ | ✅ |
| **Already in repo** | ✅ nexus-idl | ✅ | ✅ | ❌ | ❌ |
| **Tooling** | ✅ capnp compile | ✅ | ✅ | ❌ Complex | ⚠️ Custom |
| **Complexity** | 🟡 Medium | 🟢 Low | 🟢 Low | 🔴 High | 🟢 Low |

**Decision**: Cap'n Proto provides the best balance of determinism, performance, and existing infrastructure.

---

## Schema Definition

### **`tools/nexus-idl/schemas/manifest.capnp`**

```capnp

@0xf8a3b2c1d4e5f6a7;  # Unique schema ID

struct BundleManifest {
  # Schema version (for evolution)
  schemaVersion @0 :UInt8 = 1;
  
  # Core fields (v1.0)
  name @1 :Text;
  semver @2 :Text;  # SemVer string (e.g. "1.2.3")
  abilities @3 :List(Text);
  capabilities @4 :List(Text);
  minSdk @5 :Text;  # Minimum SDK version
  publisher @6 :Data;  # 32 bytes (hex decoded)
  signature @7 :Data;  # 64 bytes (Ed25519)
  
  # v1.1 additions (for digest verification)
  payloadDigest @8 :Data;  # SHA-256 (32 bytes)
  payloadSize @9 :UInt64;
  
  # Future extensions (v2+)
  # dependencies @10 :List(Dependency);
  # permissions @11 :List(Permission);
}

```text

---

## Implementation Plan

### Phase 1: Schema & Tooling (TASK-0007 v1.0)

1. **Define schema**: `tools/nexus-idl/schemas/manifest.capnp`
2. **Update `nxb-pack`**:

   ```rust

   // Input: manifest.toml (TOML)
   // Output: manifest.nxb (Cap'n Proto binary)
   fn compile_manifest(toml: &Path, output: &Path) -> Result<()> {
       let manifest = parse_toml(toml)?;
       let mut builder = capnp::message::Builder::new_default();
       let mut msg = builder.init_root::<manifest_capnp::bundle_manifest::Builder>();
       msg.set_name(&manifest.name);
       // ... set all fields
       capnp::serialize::write_message(&mut output_file, &builder)?;
   }

```text

3. **Update `bundlemgr` parser (host)**:

   ```rust

   // userspace/bundlemgr/src/manifest.rs
   pub fn parse_binary(bytes: &[u8]) -> Result<Manifest> {
       let reader = capnp::serialize::read_message(bytes, ReaderOptions::new())?;
       let msg = reader.get_root::<manifest_capnp::bundle_manifest::Reader>()?;
       Ok(Manifest {
           name: msg.get_name()?.to_string(),
           version: Version::parse(msg.get_semver()?)?,
           // ...
       })
   }

```text

4. **Update `bundlemgrd` parser (OS)**:

   ```rust

   // source/services/bundlemgrd/src/os_lite.rs
   #[cfg(nexus_env = "os")]
   fn parse_manifest_os(bytes: &[u8]) -> Result<ManifestView> {
       let reader = capnp::serialize::read_message_from_flat_slice(
           bytes,
           ReaderOptions::new()
       )?;
       let msg = reader.get_root::<manifest_capnp::bundle_manifest::Reader>()?;
       // Return view (no allocation)
   }

```text

5. **Migrate test fixtures**:

   ```rust

   // userspace/exec-payloads/build.rs
   fn main() {
       compile_manifest_toml_to_nxb("hello.manifest.toml", "hello.manifest.nxb");
   }
   
   // userspace/exec-payloads/src/hello_elf.rs
   pub const HELLO_MANIFEST: &[u8] = include_bytes!("hello.manifest.nxb");

```text

### Phase 2: v1.1 Fields (TASK-0034 or later)

6. **Add digest/size fields** to schema (already defined above)
7. **Update `nxb-pack`** to compute SHA-256(payload.elf)
8. **Update parsers** to validate digest on install

---

## Migration Strategy

### Backward Compatibility

**None required**: This is a breaking change, but:
- No production deployments exist yet
- All in-tree bundles will be rebuilt with new format
- Old format (JSON/TOML) is removed entirely

### Rollout

1. **PR 1**: Schema + tooling (`nxb-pack`, parsers)
2. **PR 2**: Migrate test fixtures (`exec-payloads`)
3. **PR 3**: Update docs (`nxb.md`, `04-bundlemgr-manifest.md`)
4. **PR 4**: Remove old TOML parser code

---

## Consequences

### Positive

- ✅ **Single source of truth**: `manifest.nxb` is canonical
- ✅ **Deterministic**: Same data → same binary (signable)
- ✅ **Fast**: Zero-copy parsing in OS mode
- ✅ **Versionable**: Schema can evolve without breaking old parsers
- ✅ **Consistent**: Same format in tooling, host tests, and OS

### Negative

- ❌ **Not human-readable**: Binary format (but TOML source is editable)
- ❌ **Tooling dependency**: Requires `capnp` compiler
- ❌ **Migration effort**: All existing bundles must be rebuilt

### Neutral

- 🟡 **Complexity**: Medium (Cap'n Proto is well-documented, but adds a layer)

---

## Alternatives Considered

### 1. **JSON (like Fuchsia)**

**Rejected**: Not deterministic (whitespace, key order). Would need "canonical JSON" hacks.

### 2. **TOML (current host parser)**

**Rejected**: Same issues as JSON (whitespace, key order). Not designed for signing.

### 3. **Binary XML (like Android AXML)**

**Rejected**: Too complex. Android-specific format, no existing Rust tooling.

### 4. **Custom TLV (Type-Length-Value)**

**Rejected**: Would work, but reinvents the wheel. No schema evolution story.

### 5. **Protobuf**

**Considered**: Similar to Cap'n Proto, but:
- Cap'n Proto is already in repo (`nexus-idl`)
- Cap'n Proto has better zero-copy support
- Cap'n Proto has cleaner no_std story

---

## Related Decisions

- **ADR-0009**: Bundle Manager Architecture (defines manifest role)
- **ADR-0017**: Service Architecture (host-first testing)
- **TASK-0007**: Updates & Packaging v1.0 (first use of manifest.nxb)
- **TASK-0034**: Delta updates v1 (adds digest/size fields in v1.1)

---

## References

- Cap'n Proto: <https://capnproto.org/>
- Cap'n Proto Rust: <https://github.com/capnproto/capnproto-rust>
- Android AXML: <https://android.googlesource.com/platform/frameworks/base/+/master/tools/aapt2/>
- Fuchsia packages: <https://fuchsia.dev/fuchsia-src/concepts/packages/package>

---

## Approval

**Approved by**: @runtime  
**Date**: 2026-01-15  
**Implementation tracking**: TASK-0007 (v1.0), TASK-0034 (v1.1)
