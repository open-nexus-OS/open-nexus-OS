#!/bin/bash
qemu-system-riscv64 \
  -machine virt \
  -cpu rv64 \
  -smp 4 \
  -m 8G \
  -kernel redox/target/riscv64-redox/debug/kernel \
  -drive file=nexus.img,format=raw,id=disk \
  -device virtio-blk-device,drive=disk \
  -serial mon:stdio \
  -display gtk,gl=on \
  -device virtio-gpu-pci \
  -device virtio-keyboard \
  -device virtio-mouse