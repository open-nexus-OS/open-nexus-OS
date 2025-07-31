#!/bin/bash
set -e

# =============================================================================
# Redox OS QEMU Runner Script (Official Make Integration)
#
# This script launches Redox OS using the official "make qemu" command
# provided by the Redox build system. It assumes that Redox has already
# been built successfully and will automatically run the correct QEMU
# configuration based on the selected architecture and config.
#
# 📁 Expected Directory Structure:
#   ~/open-nexus-OS/redox/       <-- Redox git repo with build output
#
# ✅ Usage:
#   ./run-qemu.sh
#
# Note: Run from project root (e.g. ~/open-nexus-OS/)
# =============================================================================

# Color output
INFO="\033[1;34m[INFO]\033[0m"
ERROR="\033[1;31m[ERROR]\033[0m"

# Configuration
REDOX_DIR="redox"

# Check for Redox directory
if [ ! -d "$REDOX_DIR" ]; then
  echo -e "$ERROR Redox directory '$REDOX_DIR' not found!"
  echo "Please run the initial setup script first to clone the Redox repository and then the build script."
  exit 1
fi

# Run QEMU using Redox's Makefile
echo -e "$INFO Starting Redox OS using 'make qemu'..."
cd "$REDOX_DIR"
make qemu
