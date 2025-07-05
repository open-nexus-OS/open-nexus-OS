Eine moderne RISC-V Distribution mit Cosmic Desktop Environment

Open-Nexus-OS kombiniert die Sicherheit von Redox mit der Eleganz von Cosmic Desktop in einer RISC-V optimierten Distribution.

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
├── nexus/      # Nexus-spezifische Komponenten
├── scripts/    # Build- und Hilfsskripte
└── docs/       # Dokumentation
🤝 Mitwirken
Wir freuen uns über Beiträge! Bitte lesen Sie:

CONTRIBUTING.md

Verhaltenskodex

📜 Lizenz
Open-Nexus-OS ist unter GPL-3.0 lizenziert, enthält aber Komponenten mit anderen Lizenzen:

Redox OS: Apache 2.0

Cosmic Desktop: GPL

Details siehe LICENSE und NOTICE.
