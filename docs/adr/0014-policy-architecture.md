# ADR-0014: Policy Architecture

## Status
Accepted

## Context
The policy system provides capability-based access control for Open Nexus OS. It determines which subjects (users, services, applications) have access to which capabilities based on policy documents.

## Decision
Implement a directory-based policy system with the following architecture:

### Core Components
- **Policy Document**: TOML-based policy files with subject-to-capability mappings
- **Policy Loader**: Directory scanning and policy merging
- **Capability Checker**: Runtime capability validation
- **Subject Canonicalization**: Consistent subject name handling

### Policy Format
- **File Format**: TOML files in policy directory
- **Subject Mapping**: Maps subject names to capability lists
- **Capability Lists**: Arrays of capability names
- **Directory Merging**: Later files override earlier ones

### Security Model
- **Capability-Based**: Access control based on declared capabilities
- **Subject Validation**: Subject names are canonicalized
- **Policy Integrity**: Policy files are validated on load
- **Denial Reporting**: Specific missing capabilities reported

### Integration Points
- **Service Manager**: Check service capabilities
- **Bundle Manager**: Validate bundle capability requirements
- **System Services**: Enforce capability checks
- **Application Runtime**: Runtime capability validation

## Consequences
- **Positive**: Fine-grained access control
- **Positive**: Policy changes without code changes
- **Positive**: Clear denial information for debugging
- **Negative**: Policy complexity can grow
- **Negative**: Directory scanning overhead

## Implementation Notes
- Policy directory contains multiple TOML files
- Subject names are canonicalized (lowercase, trimmed)
- Capability names are canonicalized for consistent matching
- Policy merging allows incremental policy updates
- Denial information includes specific missing capabilities


