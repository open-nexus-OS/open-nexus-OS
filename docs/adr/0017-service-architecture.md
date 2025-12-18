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

### Architecture Principles
1. **Service Discovery**: Services register with samgrd for discovery
2. **IPC Communication**: Cap'n Proto-based messaging via nexus-ipc
3. **Lifecycle Management**: Standard startup, shutdown, and health check patterns
4. **Capability-Based Access**: Policy-driven access control via policyd
5. **Host/OS Backends**: Support for both host development and OS deployment

### Service Patterns
- **Daemon Pattern**: Long-running background services
- **Request-Response**: Synchronous service calls
- **Event-Driven**: Asynchronous event handling
- **State Management**: Persistent service state
- **Error Handling**: Structured error propagation

### Invariants
- Services must register with samgrd before accepting requests
- IPC messages must follow Cap'n Proto schema definitions
- Service state must be consistent across restarts
- Capability checks must be performed for all privileged operations
- Host and OS backends must provide equivalent functionality

## Consequences
- **Positive**: Consistent service development, easier maintenance, clear separation of concerns
- **Negative**: Increased complexity in service communication, stricter adherence to patterns
- **Risks**: Service discovery failures, IPC communication errors, capability enforcement bypass











