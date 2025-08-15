#!/bin/bash

# Pfad zum Projektordner (anpassen falls nötig)
PROJECT_DIR="open-nexus-os"

echo "Suche nach 'orbital' im Projektverzeichnis $PROJECT_DIR ..."

grep -rn --color=always "orbital" "$PROJECT_DIR" | tee orbital_references.log

echo
echo "Suche beendet. Gefundene Referenzen sind in orbital_references.log gespeichert."