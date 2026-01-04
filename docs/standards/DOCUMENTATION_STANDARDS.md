# Documentation Standards

## Overview
This document defines the documentation standards for Open Nexus OS, including CONTEXT headers, ADR references, and CODEOWNERS structure.

## Quick Reference

### CONTEXT Header Format
```rust
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: [Brief description]
//! OWNERS: @team-name
//! STATUS: Functional/Experimental/Placeholder/Deprecated
//! API_STABILITY: Stable/Unstable
//! TEST_COVERAGE: [Number] tests or "No tests"
//! ADR: docs/adr/[number]-[module-name]-architecture.md
```

### Status Categories
- **Functional**: Fully implemented and tested
- **Experimental**: Works but API may change
- **Placeholder**: Only stubs, not functional
- **Deprecated**: Will be removed/replaced

### Team Responsibilities
- **@kernel-team**: Kernel implementation and architecture
- **@runtime**: Userspace libraries, services, and runtime components
- **@tools-team**: Build tools and development utilities

## Detailed Standards

### Standard Format
All Rust source files must include a CONTEXT header at the top with the following structure:

```rust
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: [Brief description of module purpose]
//! OWNERS: @team-name
//! STATUS: Functional/Experimental/Placeholder/Deprecated
//! API_STABILITY: Stable/Unstable
//! TEST_COVERAGE: [Number] tests or "No tests"
//! ADR: docs/adr/[number]-[module-name]-architecture.md
```

### Header Fields

#### CONTEXT
- **Purpose**: Brief description of what the module does
- **Format**: Single line, descriptive
- **Examples**: 
  - "Virtual file system client library"
  - "Service manager domain library for service registration and discovery"
  - "Integration tests for bundle manager CLI functionality"

#### OWNERS
- **Purpose**: Defines code ownership and review responsibility
- **Format**: @team-name or @username
- **Teams**: @kernel-team, @runtime, @tools-team

#### STATUS
- **Purpose**: Indicates implementation maturity
- **Values**:
  - `Functional`: Fully implemented and tested
  - `Experimental`: Works but API may change
  - `Placeholder`: Only stubs, not functional
  - `Deprecated`: Will be removed/replaced

#### API_STABILITY
- **Purpose**: Indicates API stability for external consumers
- **Values**:
  - `Stable`: API won't change without major version bump
  - `Unstable`: API may change in minor versions

#### TEST_COVERAGE
- **Purpose**: Documents actual test coverage
- **Format**: "[Number] tests" or "No tests"
- **Examples**: "3 unit tests", "1 integration test", "No tests"

#### ADR
- **Purpose**: References architectural decision record
- **Format**: `docs/adr/[number]-[module-name]-architecture.md`
- **Examples**:
  - `docs/adr/0002-nexus-loader-architecture.md`
  - `docs/adr/0009-bundle-manager-architecture.md`

### Extended Format for Complex Modules
For critical modules with many dependencies, additional sections may be included:

```rust
//! CONTEXT: [Description]
//! OWNERS: @team
//! STATUS: [Status]
//! API_STABILITY: [Stability]
//! TEST_COVERAGE: [Coverage]
//!
//! PUBLIC API:
//!   - [Function signatures and descriptions]
//!
//! SECURITY INVARIANTS:
//!   - [Critical security assumptions]
//!
//! ERROR CONDITIONS:
//!   - [Error types and conditions]
//!
//! DEPENDENCIES:
//!   - [External dependencies]
//!
//! FEATURES:
//!   - [Feature list]
//!
//! TEST SCENARIOS:
//!   - [Test descriptions]
//!
//! ADR: docs/adr/[number]-[module-name]-architecture.md
```

### File Type Specific Formats

#### Library Files (lib.rs)
Library files should include PUBLIC API and DEPENDENCIES sections:

```rust
//! CONTEXT: [Library description]
//! OWNERS: @team
//! STATUS: [Status]
//! API_STABILITY: [Stability]
//! TEST_COVERAGE: [Coverage]
//! 
//! PUBLIC API:
//!   - [Main types/functions]: [Description]
//! 
//! DEPENDENCIES:
//!   - [External crates]: [Purpose]
//! 
//! ADR: docs/adr/[number]-[module-name]-architecture.md
```

#### Test Files (tests/*.rs)
Test files should include TEST_SCOPE, TEST_SCENARIOS, and DEPENDENCIES sections:

```rust
//! CONTEXT: [Test type] tests for [module] [functionality]
//! OWNERS: @team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: [Number] tests
//! 
//! TEST_SCOPE:
//!   - [Functionality 1]
//!   - [Functionality 2]
//!   - [Error handling]
//! 
//! TEST_SCENARIOS:
//!   - test_[name](): [What is tested]
//! 
//! DEPENDENCIES:
//!   - [Mock/Stub dependencies]
//!   - [Test data/fixtures]
//! 
//! ADR: docs/adr/[relevant].md
```

#### CLI Files (cli.rs, main.rs)
CLI files should include PUBLIC API section with CLI interface:

```rust
//! CONTEXT: [CLI description]
//! OWNERS: @team
//! STATUS: [Status]
//! API_STABILITY: [Stability]
//! TEST_COVERAGE: [Coverage]
//! 
//! PUBLIC API:
//!   - help() -> &'static str: CLI usage string
//!   - execute(args: &[&str]) -> String: CLI execution
//!   - run(): Daemon entry point
//! 
//! DEPENDENCIES:
//!   - std::env::args: CLI argument processing
//! 
//! ADR: docs/adr/[number]-[module-name]-architecture.md
```

#### Kernel Files (kernel/*.rs)
Kernel files use a specialized format with INVARIANTS and DEPENDS_ON:

```rust
//! CONTEXT: [Kernel module description]
//! OWNERS: @kernel-team
//! PUBLIC API: [Main functions and types]
//! DEPENDS_ON: [External dependencies]
//! INVARIANTS: [Critical kernel invariants]
//! ADR: docs/adr/[number]-[module-name]-architecture.md
```

#### Library Files (libs/*.rs)
Library files follow the standard format with PUBLIC API and DEPENDENCIES:

```rust
//! CONTEXT: [Library description]
//! OWNERS: @runtime
//! STATUS: [Status]
//! API_STABILITY: [Stability]
//! TEST_COVERAGE: [Coverage]
//! 
//! PUBLIC API:
//!   - [Main types/functions]: [Description]
//! 
//! DEPENDENCIES:
//!   - [External crates]: [Purpose]
//! 
//! ADR: docs/adr/[number]-[module-name]-architecture.md
```

#### Service Files (services/*.rs)
Service files use a specialized format with PUBLIC API and DEPENDS_ON:

## Change management (keep docs in sync)

When a change affects architecture, security boundaries, ABI, IPC, capabilities, loader, or testing
workflow, documentation updates are part of “done”.

### Logging + marker discipline (default)

When introducing or modifying logs/markers:

- Prefer the **shared logging facade** (`nexus-log` / `log_*` macros) and centralized marker helpers.
- Keep output **deterministic** and **honest** (“no fake success”): only emit `*: ready` / `SELFTEST: ... ok` when the behavior actually happened.
- Avoid new ad-hoc UART print loops except as a **panic/trap floor** (where allocation/formatting is unsafe).

### Required updates (rule of thumb)

- **ADRs (`docs/adr/`)**: update the relevant ADR(s) with a short **Current state** section if the
  implementation is transitional, and add cross-links to the canonical RFC if one exists.
- **RFCs (`docs/rfcs/`)**: update the RFC that specifies the contract/ABI/semantics (e.g. RFC‑0005
  for IPC + capabilities).
- **Testing (`docs/testing/`)**: update the testing methodology and marker/CI expectations if the
  change affects how we validate correctness (new markers, new E2E requirements, new negative tests).
- **Headers (source file CONTEXT headers)**: update `STATUS`, `TEST_COVERAGE`, and invariants when
  the implementation or its risk profile changes.

### Contradictions and drift (must be called out)

If you notice a contradiction between:

- ADR/RFC text and the current implementation, or
- two different docs describing incompatible behavior,

then:

1. Add a small **“Current state / Drift”** note in the affected ADR/RFC (don’t silently rewrite history).
2. Open a discussion: record the competing concepts and choose the intended direction.

This is especially important for kernel syscall error semantics, capability transfer rules, and IPC
transport behavior, where “almost correct” can still be insecure.

```rust
//! CONTEXT: [Service description]
//! OWNERS: @services-team
//! PUBLIC API: [Main service functions]
//! DEPENDS_ON: [Service dependencies]
//! INVARIANTS: [Service-specific invariants]
//! ADR: docs/adr/[number]-[module-name]-architecture.md
```

#### Assembly Files (*.S)
Assembly files use a specialized format with detailed comments:

```assembly
/* SPDX-License-Identifier: Apache-2.0 */
/* [Function description]
 *
 * Responsibilities:
 *   - [Primary responsibility 1]
 *   - [Primary responsibility 2]
 *   - [Primary responsibility 3]
 *
 * Notes:
 *   - [Important implementation notes]
 *   - [Architecture-specific details]
 *   - [Performance considerations]
 */
```

## ADR (Architectural Decision Records)

### Structure
Each ADR follows this template:

```markdown
# ADR-[number]: [Title]

## Status
Accepted/Proposed/Deprecated

## Context
[Background and problem statement]

## Decision
[The change that is being proposed or made]

## Consequences
[What becomes easier or more difficult to do and any risks introduced by this change]
```

### Naming Convention
- Format: `[number]-[module-name]-architecture.md`
- Examples:
  - `0002-nexus-loader-architecture.md`
  - `0009-bundle-manager-architecture.md`
  - `0014-policy-architecture.md`

## CODEOWNERS Structure

### File Location
`.github/CODEOWNERS`

### Current Structure
```bash
# CODEOWNERS - define reviewers/owners for kernel paths
# NOTE: Replace @kernel-team with actual GitHub team or usernames

/source/kernel/neuron/** @kernel-team
/source/libs/** @runtime
/source/services/** @runtime
/userspace/** @runtime
/docs/ARCHITECTURE.md @kernel-team
/docs/adr/** @runtime
```

### Team Responsibilities
- **@kernel-team**: Kernel implementation and architecture
- **@runtime**: Userspace libraries, services, and runtime components
- **@tools-team**: Build tools and development utilities

## Implementation Guidelines

### For New Files
1. Add CONTEXT header with all required fields
2. Determine appropriate ADR reference
3. Set accurate STATUS and TEST_COVERAGE
4. Ensure OWNERS field matches CODEOWNERS

### For Existing Files
1. Update CONTEXT header to match standard format
2. Replace generic ADR references with specific ones
3. Verify STATUS reflects actual implementation
4. Count actual tests for TEST_COVERAGE

### For Documentation Updates
1. Update ADR when architectural decisions change
2. Update CONTEXT headers when implementation changes
3. Update CODEOWNERS when team responsibilities change
4. Maintain consistency across all files

## Quality Assurance

### CI Checks
- CONTEXT header presence in `lib.rs` and `main.rs` files
- ADR reference validity
- Copyright header presence
- Consistent formatting

### Review Process
- All changes must maintain documentation standards
- ADR references must point to existing documents
- STATUS must accurately reflect implementation
- TEST_COVERAGE must be verifiable

## Tools and Automation

### Header Validation
Use CI to check for:
- Required header fields
- Valid ADR references
- Consistent formatting
- Copyright compliance

### Documentation Generation
- ADRs can be generated from CONTEXT headers
- API documentation can be extracted from PUBLIC API sections
- Test coverage can be validated against TEST_COVERAGE claims

## Maintenance

### Regular Updates
- Review STATUS accuracy quarterly
- Update TEST_COVERAGE when tests are added/removed
- Validate ADR references during code reviews
- Update OWNERS when team responsibilities change

### Migration Guide
When updating existing files:
1. Preserve existing information
2. Convert to standard format
3. Verify accuracy of all fields
4. Update related ADRs if needed

## Examples

### Simple Module
```rust
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Clipboard storage and management
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test, 1 integration test
//! ADR: docs/adr/0008-clipboard-architecture.md
```

### Library Module (lib.rs)
```rust
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Device identity and cryptographic signing support
//! OWNERS: @runtime
//! STATUS: Functional (host backend), Placeholder (OS backend - provide stubs that will be wired later)
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 unit tests, 1 integration test
//! 
//! PUBLIC API:
//!   - Identity: Device identity with signing capabilities
//!   - DeviceId: Stable device identifier
//!   - IdentityError: Error type for identity operations
//! 
//! DEPENDENCIES:
//!   - ed25519-dalek: Digital signatures
//!   - rand_core: Random number generation
//!   - serde: JSON serialization
//!   - sha2: Cryptographic hashing
//! 
//! ADR: docs/adr/0006-device-identity-architecture.md
```

### Test Module (tests/*.rs)
```rust
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for bundle manager CLI functionality
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 CLI tests
//! 
//! TEST_SCOPE:
//!   - Bundle installation flow
//!   - Bundle removal flow
//!   - CLI command execution
//!   - Ability registrar integration
//! 
//! TEST_SCENARIOS:
//!   - test_install_flow(): Test bundle installation via CLI
//!   - test_remove_flow(): Test bundle removal via CLI
//! 
//! DEPENDENCIES:
//!   - bundlemgr::execute: CLI execution function
//!   - StubRegistrar: Mock ability registrar for testing
//!   - Test bundle files (apps/test-signed.nxb)
//! 
//! ADR: docs/adr/0009-bundle-manager-architecture.md
```

### CLI Module (cli.rs)
```rust
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Command-line interface for bundle manager service
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 CLI tests
//! 
//! PUBLIC API:
//!   - help() -> &'static str: CLI usage string
//!   - execute(args: &[&str]) -> String: CLI execution
//!   - run(): Daemon entry point
//! 
//! DEPENDENCIES:
//!   - std::env::args: CLI argument processing
//! 
//! ADR: docs/adr/0009-bundle-manager-architecture.md
```

## Conclusion

These documentation standards ensure consistency, clarity, and maintainability across the Open Nexus OS codebase. They provide clear guidelines for both human developers and AI agents working with the codebase.
