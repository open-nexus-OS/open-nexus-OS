#!/bin/sh
set -eu

QEMU_BIN="qemu-system-riscv64"
MACHINE="virt"
SMP="-smp 4"
MEM="-m 512M"
SERIAL="-serial mon:stdio"
KERNEL_IMAGE=${1:-target/riscv64imac-unknown-none-elf/release/neuron-boot}

if [ "${DEBUG:-0}" = "1" ]; then
  GDB_FLAGS="-s -S"
else
  GDB_FLAGS=""
fi

exec ${QEMU_BIN} -machine ${MACHINE} ${SMP} ${MEM} -nographic ${SERIAL} \
  ${GDB_FLAGS} -kernel "${KERNEL_IMAGE}"
