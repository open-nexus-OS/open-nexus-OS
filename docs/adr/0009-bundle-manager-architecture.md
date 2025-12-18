# ADR-0009: Bundle Manager Architecture

## Status
Accepted

## Context
The bundle manager is responsible for installing, managing, and querying application bundles in the Open Nexus OS ecosystem. It handles bundle validation, signature verification, and integration with the service manager for ability registration.

## Decision
Implement a host-first bundle manager with the following architecture:

### Core Components
- **Manifest Parser**: TOML-based manifest parsing with validation
- **Service Layer**: Bundle installation, removal, and querying
- **CLI Interface**: Command-line interface for bundle operations
- **Signature Verification**: Cryptographic signature validation
- **Publisher Validation**: Publisher identity verification

### Bundle Format
- **File Extension**: `.nxb` (Nexus Bundle)
- **Manifest Format**: TOML with required fields (name, version, publisher, signature)
- **Signature**: Base64-encoded cryptographic signature
- **Capabilities**: Required system capabilities declaration

### Security Model
- **Signature Verification**: All bundles must be cryptographically signed
- **Publisher Validation**: Publisher identity must be validated
- **Capability Requirements**: Bundle capabilities must be declared
- **Path Validation**: Prevent directory traversal attacks

### Integration Points
- **Service Manager**: Register bundle abilities after installation
- **Package File System**: Access bundle contents
- **Policy System**: Enforce capability-based access control

## Consequences
- **Positive**: Secure bundle management with cryptographic verification
- **Positive**: Clear separation of concerns between components
- **Positive**: Host-first development enables testing without OS
- **Negative**: Requires signature infrastructure
- **Negative**: TOML parsing adds dependency

## Implementation Notes
- Host backend uses file system for bundle storage
- OS backend will integrate with kernel VMO system
- CLI provides install, remove, query, and help commands
- Manifest validation includes field type checking and warnings











