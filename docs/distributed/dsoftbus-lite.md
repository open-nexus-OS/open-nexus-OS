# DSoftBus-lite overview

DSoftBus-lite provides the userland-only distributed fabric used by Open Nexus
OS services. It focuses on host-first development: all transports are
implemented in Rust, and avoid any kernel dependencies.

**Hybrid contract (aligns with VISION + RFC-0005):**

- **Control plane (target)**: typed IDL frames (Cap'n Proto) for stable, evolvable service protocols.
- **Bring-up allowance**: early OS milestones may use compact, versioned **byte-frame** protocols over the
  same authenticated stream to keep dependencies and debugging surface small.
- **Data plane**: large payloads stay out-of-band (VMO/filebuffer style). Over DSoftBus this starts as
  chunked transfers with explicit bounds; later it can map to real VMOs without copying.

The daemon is responsible for three major tasks:

This is aligned with the project vision: distributed behavior is layered in userland (`softbusd`
later), while the kernel stays minimal and capability-based (see `docs/agents/VISION.md`).

1. **Discovery** – each node announces its device identifier, published
   services, and listening port. The host backend uses an in-process registry so
   tests can run without sockets; the OS build will swap in a multicast-based
   discovery layer once networking is available.
2. **Authenticated session establishment** – peers authenticate using Noise XK
   handshakes seeded with static keys derived from the identity daemon. During
   the handshake both sides sign an attestation covering their Noise static
   keys, providing proof of possession of the Ed25519 device key material.
3. **Reliable stream framing** – once the Noise transport is ready a framed
   stream carries Cap'n Proto request/response traffic. Each frame identifies a
   logical channel, letting the daemon multiplex service protocols such as
   `samgr` and `bundlemgr` over a single encrypted TCP connection.

Large payloads (bundle artifacts, images, etc.) remain out-of-band. On the host
we emulate kernel VMO handles by sending the bytes over a dedicated channel and
stashing them in the bundle manager's artifact store before the install request
is forwarded. Kernel integration will eventually map these handles to real VMOs
and avoid copying.

## Host vs OS split

The `userspace/dsoftbus` crate exposes the high level traits used by the daemon.
Two backends exist today:

- `cfg(nexus_env = "host")` implements discovery, Noise handshakes, and framed
  streams using TCP loopback sockets. The discovery registry is process-local so
  multiple nodes can run inside a single integration test.
- `cfg(nexus_env = "os")` exposes stub modules with `todo!()` markers. They
  compile as placeholders until the kernel gains socket support and the
  transport can be wired to virtio-net.

By keeping the kernel unaware of IDL parsing or Cap'n Proto framing we preserve
its minimal trusted computing base. Only the userland daemon deals with schema
serialization and policy decisions.
