#!/bin/bash
set -e

# RISC-V Toolchain Pfad
export RISCV=/opt/riscv
export PATH=$PATH:$RISCV/bin

# Redox für RISC-V bauen
cd redox
git checkout master
make clean
make -j$(nproc) ARCH=riscv64

# Cosmic für RISC-V bauen
cd ../cosmic
git checkout release
cargo build --release --target=riscv64gc-unknown-none-elf

# Nexus-Integration bauen
cd ../nexus
cargo build --release