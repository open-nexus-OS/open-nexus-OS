# Why We Need Another OS: The Vision of a Unified Open Ecosystem

## The Next Evolution of Computing

We're building an operating system where devices become **virtualized resources** within a self-organizing network. Your workflow is no longer tied to a single device—your watch extends your desktop, your EV integrates with your smart home—enabled by open standards, not closed ecosystems.

[More Info](https://github.com/open-nexus-OS/open-nexus/wiki)

---

✨ **Planned Features**  
🚀 Redox Microkernel (Apache 2.0 licensed)  
🏗️ RISC-V First Design  
🔄 Easy Development with QEMU  
📦 Modular Architecture  

---

## ⚡ Quick Start

### Requirements

- Linux (not WSL; ideally Arch, Ubuntu, or Fedora)

---

## 🔧 Installation

```bash
cd ~
git clone https://github.com/open-nexus-OS/open-nexus-OS.git
````

---

## 🧰 Initial Setup

```bash
make initial-setup
```

During the setup, you will be asked to choose a QEMU version.
✅ Please select **qemu-full**.

You will also be prompted to choose a Podman container runtime:
✅ Choose **crun** (preferred). `runc` is also supported.

---

## 🏗️ Build

```bash
make build
```

---

## ▶️ Run

```bash
just qemu        # Manual run; respects RUN_TIMEOUT and log caps
just test-os     # Stops once success markers appear on the UART
```

Environment knobs:

- `RUN_TIMEOUT` &mdash; defaults to `30s` and is passed to GNU `timeout` so QEMU
  sessions cannot hang indefinitely.
- `RUN_UNTIL_MARKER` &mdash; set to `1` to exit early when the UART prints
  `SELFTEST: end`, `samgrd: ready`, or `bundlemgrd: ready`.
- `QEMU_LOG_MAX` / `UART_LOG_MAX` &mdash; cap diagnostic logs (default 50 MiB and
  10 MiB respectively) to avoid runaway artifacts; the runner trims via
  `tail -c` after each run.

Equivalent Make targets are available via `make qemu` and `make test-os`.

---

## 📚 Documentation

- [Project Layout Overview](docs/overview.md) &mdash; explains why each top-level
  directory exists and where to begin for kernel, service, or library work.
- [Testing Methodology & Workflow](docs/testing/index.md) &mdash; host-first testing
  philosophy, required tooling, and the step-by-step checklist for changes.

---

## 📂 Project Structure

```
open-nexus-OS/
├── config/      # Shared linting/toolchain configuration
├── docs/        # Documentation (overviews, testing guides, RFCs)
├── kernel/      # NEURON kernel library (`neuron`) and boot binary (`neuron-boot`)
├── podman/      # Container definitions matching CI
├── recipes/     # Reproducible build & environment scripts
├── scripts/     # QEMU runners, setup helpers, log trimming utilities
├── source/      # Services and applications (thin daemons adapt IPC to libs)
├── tools/       # Developer tooling, generators, lint helpers
└── userspace/   # Host-first domain libraries shared across the system
```

---

## 🤝 Contribute

We welcome contributions!
Please read our [Contributing Guide](https://github.com/open-nexus-OS/open-nexus/wiki/Contributing).

### Code of Conduct

We expect respectful and inclusive communication from all contributors.

---

## 📜 License

Open-Nexus-OS is licensed under Apache 2.0.
Some components are under different licenses:

- Redox OS: Apache 2.0

See [LICENSE](https://github.com/open-nexus-OS/open-nexus-OS/blob/main/LICENSE) and [NOTICE](https://github.com/open-nexus-OS/open-nexus-OS/blob/main/NOTICE) for full details.

---

💡 **Tip:** Join our GitHub Discussions for questions and ideas!

---
