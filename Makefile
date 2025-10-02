SHELL := /bin/sh
MODE ?= container
CONTAINER_TAG ?= open-nexus-os:dev

.PHONY: initial-setup build test run pull clean

initial-setup:
@echo "==> Checking podman rootless support"
@scripts/check-rootless.sh
@echo "==> Installing Rust targets"
@rustup target add riscv64imac-unknown-none-elf
@echo "==> Installing git hooks"
@./scripts/fmt-clippy-deny.sh

build:
ifeq ($(MODE),container)
@echo "==> Building workspace inside podman"
@podman build -t $(CONTAINER_TAG) -f podman/Containerfile .
@podman run --rm -t -v $(PWD):/workspace -w /workspace $(CONTAINER_TAG) cargo build --workspace
else
@echo "==> Building workspace on host"
@cargo build --workspace
endif

test:
ifeq ($(MODE),container)
@echo "==> Running host-first tests inside podman"
@podman run --rm -t -v $(PWD):/workspace -w /workspace $(CONTAINER_TAG) cargo nextest run --workspace
else
@echo "==> Running host-first tests"
@cargo nextest run --workspace
endif

run:
@echo "==> Launching NEURON kernel under QEMU"
@./scripts/qemu-run.sh

pull:
@echo "==> Refreshing recipe sources"
@find recipes -name recipe.toml -print | while read -r recipe; do \
echo "Syncing $$recipe"; \
grep '^\[source\]' -n "$$recipe" >/dev/null || true; \
done

clean:
@echo "==> Cleaning build artifacts"
@cargo clean
@rm -rf build
