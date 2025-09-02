#!/bin/bash
# =============================================================================
# open nexus OS Clean Script
#
# This script cleans up the build environment for open nexus OS.
# It removes old build artifacts and caches to ensure a fresh build.
# =============================================================================
#
# 💡 This script:
#   1. Removes old build directories
#   2. Removes old recipe builds
#   3. Deletes Redox cache
#   4. Deletes Cargo registry and git checkouts
# =============================================================================

# Error Messages with row numbers
trap 'echo -e "\033[1;31m[CRASHED]\033[0m Fehler in Zeile $LINENO (Befehl: $BASH_COMMAND)"' ERR

set -e

echo "🧹 Removes old build directories..."
rm -rf build/x86_64/desktop
rm -rf redox/build

echo "🧹 Removes old recipe builds..."
rm -rf recipes/gui/nexus/target
rm -rf recipes/gui/nexus-assets/target
rm -rf recipes/gui/nexus-utils/target

echo "🧹 Deletes Redox Cache..."
rm -rf ~/.cache/redox

echo "🧹 Deletes Cargo Registry and Git Checkouts..."
rm -rf ~/.cargo/registry/index
rm -rf ~/.cargo/registry/cache
rm -rf ~/.cargo/git/checkouts

echo "  Goto Redox directory..."
cd redox

echo "🧹 Clean all recipe binaries for complete rebuild"
make clean

echo "🧹 Update relibc"
make pull
touch relibc
make prefix

echo "  Go back to the previous directory..."
cd ..

echo "✅ Cleanup complete."