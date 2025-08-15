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
WARN="\033[1;33m[WARN]\033[0m"
ERROR="\033[1;31m[ERROR]\033[0m"
OK="\033[1;32m[OK]\033[0m"

# Configuration
REDOX_DIR="redox"
ARCH="x86_64"
CONFIG_NAME="desktop"
BUILD_DIR="build/$ARCH/$CONFIG_NAME"
EXTRA_IMAGE_PATH="$BUILD_DIR/extra.img"

# Check for Redox directory
if [ ! -d "$REDOX_DIR" ]; then
  echo -e "$ERROR Redox directory '$REDOX_DIR' not found!"
  echo "Please run the initial setup script first to clone the Redox repository and then the build script."
  exit 1
fi

# Run QEMU using Redox's Makefile
cd "$REDOX_DIR"

echo -e "$INFO Starting run-qemu script..."
if [ "$1" == "test" ]; then
    # Extra-Image nur anlegen, wenn es noch nicht existiert
    if [ ! -f "$EXTRA_IMAGE_PATH" ]; then
        echo -e "$INFO Creating extra disk at $EXTRA_IMAGE_PATH..."
        truncate -s 1G "$EXTRA_IMAGE_PATH"
    fi
    echo -e "$INFO Running QEMU in test mode with extra disk..."
    make qemu EXTRA_DISK="$EXTRA_IMAGE_PATH"
    exit 0
elif [ "$1" == "help" ]; then
    echo -e "Rundown of available commands:"
    echo -e "test   - Run QEMU in test mode"
    echo -e "help   - Show this help message"
    echo -e "empty  - Run QEMU in normal mode"
elif [ "$1" == "empty" ]; then
      echo -e "$INFO 'empty' is no command, just use 'make run' instead."
elif [ -n "$1" ]; then
    echo -e "$ERROR Command not found or failed!"
    echo "Please try make run help for more information."
    exit 1
else
    echo -e "$INFO Running QEMU in normal mode..."  
    echo -e "$INFO Starting Redox OS using 'make qemu'..."
    make qemu
    echo -e "$OK Redox OS should now be running in QEMU."
    echo -e "$INFO You can exit QEMU with Ctrl+Alt+G, then X."
    exit 0
fi

echo -e "$INFO Enjoy your Open Nexus OS experience!"