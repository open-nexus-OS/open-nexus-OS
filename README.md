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
make run
```

---

## 📂 Project Structure

open-nexus-OS/
├── redox/      # Redox Kernel (Apache 2.0)
├── recipes/    # Open-Nexus-specific components
├── scripts/    # Build and helper scripts
└── docs/       # Documentation

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
