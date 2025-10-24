# Nexus IDL Schemas

This directory hosts the Cap'n Proto schemas used for Nexus control-plane messaging.
Large payloads such as application bundles travel via VMOs and are referenced here by
handle identifiers; Cap'n Proto only carries the metadata required to negotiate
those transfers.

## Available Schemas

- `bundlemgr.capnp` - Bundle manager service interface
- `execd.capnp` - Execution daemon service interface  
- `identity.capnp` - Identity service interface
- `keystored.capnp` - Keystore daemon service interface
- `packagefs.capnp` - Package filesystem service interface
- `policyd.capnp` - Policy daemon service interface
- `samgr.capnp` - Service manager interface
- `vfs.capnp` - Virtual file system service interface
- `dsoftbus.capnp` - Distributed soft bus interface

## Usage

The `nexus-idl` tool generates Rust bindings from these schemas:

```bash
nexus-idl gen    # Generate bindings for all schemas
nexus-idl list   # List available schemas
```

Generated bindings are placed in `OUT_DIR` and included by the `nexus-idl-runtime` crate.
