# Why we need another OS: The vision of a Unified Open Ecosystem

## The Next Evolution of Computing

We're creating an OS where devices become **virtualized resources** in a self-organizing network. Your workflow transcends hardware - a watch extends your desktop, your EV integrates with your smart home - through open standards, not proprietary silos.

[More Infos](https://github.com/open-nexus-OS/open-nexus/wiki)

✨ Features
🚀 Redox Microkernel (Apache 2.0 Lizenz)

🖥️ Cosmic Desktop (GPL Lizenz)

🏗️ RISC-V First Design

🔄 Einfache Entwicklung mit QEMU

📦 Modulare Architektur

⚡ Schnellstart
Voraussetzungen
WSL2 (Windows) oder Linux

QEMU (≥ 6.0)

RISC-V Toolchain

Installation
bash
git clone --recurse-submodules https://github.com/open-nexus-OS/open-nexus-OS.git
cd open-nexus-OS
./scripts/setup.sh
In QEMU starten
bash
./scripts/run-qemu.sh
🛠️ Entwicklung
Build-Anleitung
bash
# Kompletter Build
make all

# Nur Kernel
make kernel

# Nur Cosmic Desktop
make cosmic
Debugging
bash
make debug   # Startet QEMU mit GDB-Server
riscv64-unknown-elf-gdb redox/target/riscv64-redox/debug/kernel
📂 Projektstruktur
text
open-nexus-OS/
├── redox/      # Redox Kernel (Apache 2.0)
├── cosmic/     # Cosmic Desktop (GPL)
├── open-nexus/      # Nexus-spezifische Komponenten
├── scripts/    # Build- und Hilfsskripte
└── docs/       # Dokumentation
🤝 Mitwirken
Wir freuen uns über Beiträge! Bitte lesen Sie:

[CONTRIBUTING](https://github.com/open-nexus-OS/open-nexus/wiki/Contributing)

Verhaltenskodex

📜 Lizenz
Open-Nexus-OS ist unter GPL-3.0 lizenziert, enthält aber Komponenten mit anderen Lizenzen:

Redox OS: Apache 2.0

Cosmic Desktop: GPL

Details siehe [LICENSE](https://github.com/open-nexus-OS/open-nexus-OS/blob/main/LICENSE) und [NOTICE](https://github.com/open-nexus-OS/open-nexus-OS/blob/main/NOTICE).

💡 Tipp: Nutzen Sie unsere Diskussionen für Fragen und Ideen!
