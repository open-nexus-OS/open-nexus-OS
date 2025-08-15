# Fix-Schritte

## 1. podman/redox-base-containerfile bearbeiten

Zeile 3:

FROM debian:stable-backports

ersetzen durch:

FROM debian:bookworm-backports

Zeile 11:

-t stable-backports

ersetzen durch:

    -t bookworm-backports

## 2. podman/redox-gdb-containerfile bearbeiten

FROM debian:stable-backports

ersetzen durch:

FROM debian:bookworm-backports

Zeile 4:

-t stable-backports

ersetzen durch:

-t bookworm-backports
