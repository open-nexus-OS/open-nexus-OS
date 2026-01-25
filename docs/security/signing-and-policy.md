# Signing and policy enforcement

Open Nexus OS pairs manifest signing with runtime capability policies. Bundle
signatures prove provenance, while the policy layer restricts which processes
may consume specific kernel capabilities.

This is part of the system's **hybrid security root**: verified boot + signed bundles/packages +
policy gating + capability enforcement (see `docs/agents/VISION.md`).

## Philosophy: Secure by default, flexible for developers

**Core principle**: Production devices are protected by strict signature verification,
while developers retain full freedom to build, test, and sideload locally without
external infrastructure.

**Key design goals**:

* **Production security**: Only trusted publishers can deploy updates to end-user devices
* **Developer freedom**: Local builds work without signing infrastructure or cloud services
* **User control**: Sideloading third-party apps is possible with explicit consent
* **Audit trail**: All installations are logged for forensics and compliance

## Policy enforcement levels

The system supports multiple enforcement modes to balance security and developer flexibility:

### Strict (Production)

**Default for end-user devices**. Only bundles signed by trusted publishers are accepted.

* Publisher keys must be in the system's trust store
* Signature verification is mandatory (no exceptions)
* Failed verification blocks installation
* Use case: Production deployments, end-user devices

**Configuration**:

```bash
# Via kernel command line
qemu ... -append "policy.enforcement=strict"

# Or via config file (future)
echo "enforcement = strict" > /etc/nexus/policy.conf
```

### Permissive (Development)

**Default for developer builds**. Self-signed bundles are accepted without warnings.

* Test keys are allowed (labeled `// SECURITY: bring-up test keys`)
* Local builds install without external signing infrastructure
* Signature verification still performed (validates format, not trust)
* Use case: Developer machines, CI/CD, local testing

**Configuration**:

```bash
# Via kernel command line
qemu ... -append "policy.enforcement=permissive"

# Automatic detection
# If built with --features dev-mode, defaults to permissive
```

### Audit-Only (Monitoring)

**For gradual rollout**. All installations are allowed but logged.

* Signature verification performed
* Failures logged but don't block installation
* Useful for testing policy changes before enforcement
* Use case: Staging environments, policy testing

**Configuration**:

```bash
qemu ... -append "policy.enforcement=audit-only"
```

### Disabled (Bring-up only)

**For early development**. No signature verification.

* Only for kernel/system bring-up
* Must be explicitly enabled at build time
* Not available in release builds
* Use case: Early boot debugging, kernel development

**Configuration**:

```bash
# Only available if built with --features bring-up
qemu ... -append "policy.enforcement=disabled"
```

## Evaluation order

1. **Bundle manifest** – `bundlemgrd` parses the signed manifest and records the
   capabilities declared by the service (`caps = [...]`).
2. **Signature verification** – Signature is verified according to enforcement level:
   * **Strict**: Must be signed by trusted publisher (in trust store)
   * **Permissive**: Signature format validated, trust not required
   * **Audit-Only**: Verified and logged, failures don't block
   * **Disabled**: Skipped
3. **Policy lookup** – `nexus-init` queries `policyd.Check(subject, requiredCaps)`
   before launching a service. Policies are assembled from the TOML files under
   `recipes/policy/`, merged lexically with later files overriding earlier
   entries.
4. **Execution** – only when `policyd` returns `allowed=true` does init request
   execution from `execd`. Denials are logged as `init: deny <name> missing=cap`.

Unknown services default to an empty allowlist, so any non-empty capability
request is denied by default.

## Extending policies

- Add a new `*.toml` file under `recipes/policy/` or update `base.toml`.
- Use lowercase service and capability names; entries are normalised.
- Later files override earlier ones. For temporary developer overrides, drop a
  `local-*.toml` file so it sorts after the base policy.
- Keep policy files in version control whenever possible so QEMU and postflight
  checks can enforce the correct allowlists.

## Local development workflow

### Building and testing locally

Developers can build and test bundles without external signing infrastructure:

```bash
# 1. Build your application
cd userspace/apps/my-app
cargo build --target riscv64imac-unknown-none-elf

# 2. Create bundle with test key (automatic in dev builds)
nxb-pack --toml manifest.toml target/my-app.elf my-app.nxb

# 3. Test locally (permissive mode accepts test keys)
make run  # Your build runs in QEMU

# 4. Install on development device
bundlemgr install my-app my-app.nxb  # Works with test signature
```

**Test keys are built-in**:

```rust
// SECURITY: bring-up test keys, NOT production custody
const DEV_PUBLISHER_KEY: [u8; 32] = [0u8; 32];
const DEV_SIGNATURE: [u8; 64] = [0u8; 64];
```

These keys are:

* Explicitly labeled in code
* Only accepted in permissive/audit-only modes
* Rejected in strict mode (production)
* Sufficient for local development and testing

### Sideloading third-party applications

Users can install applications from sources outside the official store:

#### Prerequisites

1. **Enable "Unknown Sources"** in system settings (user consent required)
2. **Download bundle** from third-party source (e.g., community repository)
3. **Review capabilities** before installation

#### Installation flow

```bash
# User downloads third-party-app.nxb

# System shows capability review dialog:
# ┌─────────────────────────────────────────┐
# │ Install "Cool Game"?                    │
# │                                         │
# │ Publisher: 0xABCD... (unverified)       │
# │                                         │
# │ Requested capabilities:                 │
# │  • gpu (graphics access)                │
# │  • network (internet access)            │
# │  • storage.user (your files)            │
# │                                         │
# │ ⚠️  This publisher is not verified.     │
# │                                         │
# │ [Cancel]  [Install Anyway]              │
# └─────────────────────────────────────────┘

# User confirms → installation proceeds
# All sideload installations are logged to logd
```

#### Audit trail

Every sideloaded installation is logged:

```rust
nexus_log::warn!(
    scope: "bundlemgrd",
    message: "sideload install",
    fields: format!(
        "bundle={}\npublisher={}\nuser_override=true\ncaps={:?}\n",
        name, hex::encode(publisher), capabilities
    )
);
```

### Enterprise deployment

Organizations can use their own signing keys:

```bash
# 1. Generate enterprise signing key
keystored generate-key --purpose signing --output enterprise.key

# 2. Sign bundles with enterprise key
nxb-pack --toml manifest.toml --sign-key enterprise.key app.nxb

# 3. Add enterprise key to device trust store
policyd add-publisher --key enterprise.pub --name "Acme Corp"

# 4. Deploy via OTA
# Devices now accept bundles signed with enterprise key
```

## Denial handling

When a service requests capabilities that are not permitted, `policyd` returns
`allowed=false` along with the missing capability names. `nexus-init` records the
failure as `init: deny <name>` and skips `execd`. Host tests and the OS
postflight harness assert that the denial path is covered by both host E2E
checks and QEMU UART markers.

## Comparison with other systems

| System | Signature Required | Self-Signed OK | Sideloading | Local Builds |
| ------ | ------------------ | -------------- | ----------- | ------------ |
| **iOS** | Yes (Apple cert) | No | Very limited | Dev account required |
| **Android** | Yes | Yes | Yes (with warning) | Yes (easy) |
| **Linux** | Optional (distro) | N/A | Yes (standard) | Yes (standard) |
| **Open Nexus** | **Yes** | **Yes (dev mode)** | **Yes (with consent)** | **Yes (explicit support)** |

## Security properties

### What signatures protect against

* **Supply chain attacks**: Malicious packages can't be injected into OTA updates
* **Transport corruption**: Integrity is verified end-to-end
* **Rollback attacks**: Version floors prevent downgrade (future: TASK-0179)
* **Unauthorized modifications**: Tampered packages are rejected

### What signatures don't protect against

* **Malicious signed packages**: If a trusted publisher is compromised, their signature is valid
* **Capability abuse**: A legitimately signed app can still misuse granted capabilities
* **Runtime exploits**: Signatures verify package integrity, not runtime behavior
* **Social engineering**: Users can still be tricked into sideloading malicious apps

**Defense-in-depth**: Signatures are one layer. Capability policies, sandboxing, and
runtime monitoring provide additional protection (see `docs/architecture/11-policyd-and-policy-flow.md`).

## Audit Trail (TASK-0008)

All policy decisions (allow/deny) are audit-logged:

- **Sink**: logd (RFC-0011) is the primary sink; UART fallback if logd unavailable
- **Record structure**: timestamp, subject_id, action, target, decision, reason
- **Bounded fields**: action ≤ 32 bytes, target ≤ 64 bytes
- **No secrets**: Audit records never contain key material or policy configuration details

Every policy-gated operation (bundle install, process exec, keystore signing) produces an audit record.

See `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` for the full audit contract.

## Policy-Gated Operations

The following operations are deny-by-default and require explicit capability:

| Operation | Required Capability | Service |
|-----------|---------------------|---------|
| Sign (Ed25519) | `crypto.sign` | keystored |
| Bundle install | `fs.verify` | bundlemgrd |
| Process exec | `proc.spawn` | execd |
| Route request | `ipc.core` | policyd (via init-lite proxy) |
| Route to execd | `route.execd` | policyd |

## QEMU Proof Markers

| Marker | Proves |
|--------|--------|
| `SELFTEST: policy deny audit ok` | A deny decision occurred AND audit record was emitted |
| `SELFTEST: policy allow audit ok` | An allow decision occurred AND audit record was emitted |
| `SELFTEST: keystored sign denied ok` | Policy-gated signing denied without required capability |

Run: `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os`
