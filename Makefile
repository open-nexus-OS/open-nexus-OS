# ==============================================================================
# Makefile for Open-Nexus-OS Project
# ------------------------------------------------------------------------------
# This Makefile provides simple commands to run your build, clean, and utility
# scripts without manually typing the full "./scripts/script-name.sh" path.
#
# Usage:
#   make <target>
#
# Examples:
#   make initial-setup -> Sets up the initial environment
#   make clean         -> This script cleans up the build environment
#   make build         -> Builds Redox with the Nexus GUI package
#   make qemu          -> Boots the NEURON kernel in QEMU (timeout aware)
#   make test-os       -> Boots NEURON and waits for UART success markers
#
# Note:
#   - The SCRIPTS_DIR variable defines where your .sh files are stored.
#   - Each target calls its corresponding shell script.
# ==============================================================================

# Location of your shell scripts
SCRIPTS_DIR := scripts
RUN_TIMEOUT ?= 30s

# Default target when running 'make' without arguments
default:
        @echo "Available targets:"
        @echo "  make initial-setup - Sets up the initial environment"
        @echo "  make clean         - Clean the build environment"
        @echo "  make build         - Build the Nexus workspace"
        @echo "  make qemu          - Run NEURON via QEMU (timeout $(RUN_TIMEOUT))"
        @echo "  make test-os       - Run NEURON self-tests with marker detection"

# ------------------------------------------------------------------------------

# Target: Initial Setup
# Description: Sets up the initial environment for the project.
initial-setup:
        bash $(SCRIPTS_DIR)/initial-setup.sh

# Target: Clean
# Description: Deletes all build artifacts, caches, and target directories.
clean:
        bash $(SCRIPTS_DIR)/clean.sh

# Target: Build Nexus
# Description: Compiles the Nexus GUI and its assets.
build:
        bash $(SCRIPTS_DIR)/build.sh

# Target: Run NEURON kernel under QEMU with bounded runtime.
qemu:
        RUN_TIMEOUT=$(RUN_TIMEOUT) bash $(SCRIPTS_DIR)/run-qemu-rv64.sh $(filter-out $@,$(MAKECMDGOALS))

# Backwards compatible alias for historical tooling.
run: qemu

# Target: Execute NEURON smoke tests and assert UART markers.
test-os:
        RUN_TIMEOUT=$(RUN_TIMEOUT) RUN_UNTIL_MARKER=1 bash $(SCRIPTS_DIR)/qemu-test.sh $(filter-out $@,$(MAKECMDGOALS))
