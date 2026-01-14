# ADR-0017: Service Architecture

## Status
Accepted

## Context
The Open Nexus OS services (`samgrd`, `bundlemgrd`, `keystored`, `identityd`, `clipboardd`, `notifd`, `searchd`, `settingsd`, `time-syncd`, `resmgrd`, `policyd`, `vfsd`, `packagefsd`, `execd`, etc.) provide system-level functionality through IPC communication. These services need a consistent architecture for discovery, communication, and lifecycle management.

## Decision
Establish a unified service architecture with the following components:

### Service Categories
- **Core Services**: samgrd (service manager), bundlemgrd (bundle manager), keystored (key management)
- **System Services**: identityd (device identity), clipboardd (clipboard), notifd (notifications)
- **Data Services**: searchd (search), settingsd (settings), time-syncd (time sync)
- **Resource Services**: resmgrd (resource manager), policyd (policy enforcement)
- **Storage Services**: vfsd (virtual file system), packagefsd (package file system)
- **Execution Services**: execd (execution manager)
- **Observability Services**: logd (log journal + crash reports)

### Architecture Principles
1. **Service Discovery**: Services register with samgrd for discovery
2. **Hybrid Communication Pattern**:
   - **Control Plane**: Small, structured IPC (Cap'n Proto for host/std, versioned byte frames for OS/os-lite)
   - **Data Plane**: Large payloads out-of-band (VMO for production, filebuffer for testing/exports)
   - See `docs/architecture/08-service-architecture-onboarding.md` for details
3. **Lifecycle Management**: Standard startup, shutdown, and health check patterns
4. **Capability-Based Access**: Policy-driven access control via policyd
5. **Host/OS Backends**: Support for both host development and OS deployment
6. **Observability**: Services use `nexus-log` facade to emit structured logs to `logd` (see RFC-0011)

### Service Patterns
- **Daemon Pattern**: Long-running background services
- **Request-Response**: Synchronous service calls
- **Event-Driven**: Asynchronous event handling
- **State Management**: Persistent service state
- **Error Handling**: Structured error propagation

### Invariants
- Services must register with samgrd before accepting requests
- Control Plane IPC must follow hybrid pattern:
  - Cap'n Proto schemas for host/std (type-safe, evolvable)
  - Versioned byte frames for OS/os-lite (minimal, deterministic)
  - IDL schema serves as documentation even when byte frames are authoritative
- Data Plane must bound inline payloads or use VMO/filebuffer for bulk:
  - Small payloads (<4KB) MAY be inline if bounded
  - Large payloads (>4KB) MUST use VMO (production) or filebuffer (testing/exports)
- Service state must be consistent across restarts
- Capability checks must be performed for all privileged operations
- Host and OS backends must provide equivalent functionality

## Consequences
- **Positive**: Consistent service development, easier maintenance, clear separation of concerns
- **Negative**: Increased complexity in service communication, stricter adherence to patterns
- **Risks**: Service discovery failures, IPC communication errors, capability enforcement bypass
