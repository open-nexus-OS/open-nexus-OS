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
#   2. Removes old recipe copies
#   3. Deletes Redox binaries
#   4. Updates relibc
# =============================================================================

# Error Messages with row numbers
trap 'echo -e "\033[1;31m[CRASHED]\033[0m Fehler in Zeile $LINENO (Befehl: $BASH_COMMAND)"' ERR

set -e

echo "  Goto Redox directory..."
cd redox

echo "🧹 Removes old build directories..."
rm -rf build

echo "🧹 Removes old recipe builds..."
rm -rf cookbook/recipes/gui/nexus
rm -rf cookbook/recipes/gui/nexus-assets
rm -rf cookbook/recipes/gui/nexus-login
rm -rf cookbook/recipes/gui/nexus-background
rm -rf cookbook/recipes/gui/nexus-launcher
rm -rf cookbook/recipes/libs/libnexus   

echo "🧹 Clean all recipe binaries for complete rebuild"
make clean

echo "🧹 Update relibc"
make pull
touch relibc
make prefix

echo "  Go back to the previous directory..."
cd ..

echo "✅ Cleanup complete."