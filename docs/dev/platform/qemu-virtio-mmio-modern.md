# QEMU: Force Modern virtio-mmio (virtio-blk)

## Context

The OS `virtio-blk` MMIO driver is correct, but **legacy virtio-mmio in QEMU leaves
`used.idx` at 0**, so persistence proofs fail. Modern virtio-mmio fixes this by
driving the used ring correctly.

## Recommended Fix (test harness default)

QEMU exposes `virtio-mmio.force-legacy` with a default of `on`. The Open Nexus OS
QEMU harness now **forces modern virtio-mmio by default** via:

- `scripts/run-qemu-rv64.sh`: adds `-global virtio-mmio.force-legacy=off`
- Opt-in legacy for debugging: `QEMU_FORCE_LEGACY=1`

If you need a QEMU binary that defaults to modern without relying on command-line
globals (e.g. for external harnesses), you can still patch QEMU to default
**force-legacy=off** for virtio-mmio.

### Dependencies

- `ninja`
- `meson`
- `python3` + `python3-venv`

### Patch

Apply the patch in `tools/qemu/virtio-mmio-force-modern.patch` to a QEMU checkout.

### Automated build (preferred)

```bash
cd /home/jenning/open-nexus-OS
./tools/qemu/build-modern.sh
export PATH="/home/jenning/open-nexus-OS/tools/qemu-src/build:$PATH"
```

### Build (example)

```bash
./configure --target-list=riscv64-softmmu
make -j"$(nproc)"
```

### Use (example)

Run the standard test (the canonical harness already forces modern virtio-mmio by default):

```bash
cd /home/jenning/open-nexus-OS
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Notes: networking determinism

Modern virtio-mmio can change timing/behavior in QEMU networking backends. The QEMU smoke harness
is intentionally **host-first, QEMU-last** and keeps some networking proofs optional by default:

- Default smoke requires `net: smoltcp iface up ...` and does not require DHCP.
- If you want to enforce DHCP proof, run with:

```bash
REQUIRE_QEMU_DHCP=1 RUN_UNTIL_MARKER=1 just test-os
```

- If you want to enforce DSoftBus proof, run with:

```bash
REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 just test-os
```

See `docs/adr/0025-qemu-smoke-proof-gating.md`.

Expected UART markers after the patch:

- `virtio-blk: mmio modern`
- `SELFTEST: statefs persist ok`
- `SELFTEST: device key persist ok`
