# ADR-0014: Policy Architecture

## Status
Accepted

## Context
The policy system provides capability-based access control for Open Nexus OS. It determines which subjects (users, services, applications) have access to which capabilities based on policy documents.

## Decision
Implement a directory-based policy system with the following architecture:

### Core Components
- **Policy Document**: TOML-based policy files with subject-to-capability mappings
- **Policy Loader**: Directory scanning and policy merging (`policyd/build.rs`)
- **Policy Evaluation Library**: `nexus-sel` — pure library for policy lookups
- **Capability Checker**: Runtime capability validation via `policyd`
- **Subject Canonicalization**: Consistent subject name handling via `service_id`
- **Audit Trail**: All allow/deny decisions logged via logd

### Policy Format
- **File Format**: TOML files in policy directory (`recipes/policy/`)
- **Subject Mapping**: Maps service names to capability lists
- **Capability Lists**: Arrays of capability names
- **Directory Merging**: Later files override earlier ones
- **Build-time compilation**: Policy TOML → Rust constants via `policyd/build.rs`

### Security Model (TASK-0008)
- **Capability-Based**: Access control based on declared capabilities
- **Service-ID Binding**: Policy decisions bind to `sender_service_id` (kernel-provided, unforgeable)
- **Deny-by-Default**: Operations without explicit policy allow are rejected
- **Single Authority**: `policyd` is the sole decision service (no duplication)
- **Audit Trail**: Every allow/deny decision produces an audit record
- **Bounded Parsing**: Inputs are size-bounded; malformed requests rejected

### Integration Points
- **Service Manager (samgrd)**: Check service capabilities
- **Bundle Manager (bundlemgrd)**: Validate bundle capability requirements
- **Exec Daemon (execd)**: Authorize process spawn via policyd
- **Keystore (keystored)**: Policy-gated signing operations
- **Init-lite**: Proxy policy checks during early boot

## Consequences
- **Positive**: Fine-grained access control
- **Positive**: Policy changes without code changes
- **Positive**: Clear denial information for debugging
- **Positive**: Complete audit trail of all security decisions
- **Negative**: Policy complexity can grow
- **Negative**: Build-time compilation adds rebuild on policy change

## Implementation Notes
- Policy directory contains multiple TOML files (`recipes/policy/`)
- Subject names are canonicalized (lowercase, trimmed) and converted to `service_id`
- Capability names are canonicalized for consistent matching
- Policy merging allows incremental policy updates
- Denial information includes specific missing capabilities
- Audit records emitted via logd (or UART fallback)

## Related RFCs
- RFC-0015: Policy Authority & Audit Baseline v1 (contract + proof gates)
