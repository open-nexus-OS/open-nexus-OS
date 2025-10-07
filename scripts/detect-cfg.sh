#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/.." && pwd)
CONFIG_FILE="${REPO_ROOT}/.cargo/config.toml"

mapfile -t CANDIDATES < <(python3 <<'PY'
import os
import pathlib
import re
import shlex
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
config_path = root / ".cargo" / "config.toml"
pattern = re.compile(r"--cfg(?:=|\s+)(?:\"([^\"]+)\"|'([^']+)'|([^\s,]+))")

candidates: list[str] = []

def add_flag(flag: str) -> None:
    flag = flag.strip()
    if not flag:
        return
    flag = flag.replace('\\"', '"').replace("\\'", "'")
    flag = flag.strip("'")
    while flag.endswith('"') and flag.count('"') % 2 == 1:
        flag = flag[:-1]
    while flag.startswith('"') and flag.count('"') % 2 == 1:
        flag = flag[1:]
    if flag not in candidates:
        candidates.append(flag)


def parse_sequence(seq) -> None:
    if isinstance(seq, str):
        parse_text(seq)
        return
    if isinstance(seq, (list, tuple)):
        for idx, item in enumerate(seq):
            if isinstance(item, str):
                if item == "--cfg" and idx + 1 < len(seq):
                    add_flag(seq[idx + 1])
                elif item.startswith("--cfg="):
                    add_flag(item.split("=", 1)[1])
                else:
                    parse_text(item)
            elif isinstance(item, (list, tuple)):
                parse_sequence(item)
            elif isinstance(item, dict):
                parse_mapping(item)
        return
    if isinstance(seq, dict):
        parse_mapping(seq)


def parse_mapping(mapping: dict) -> None:
    for value in mapping.values():
        if isinstance(value, (list, tuple, dict, str)):
            parse_sequence(value)


def parse_text(text: str) -> None:
    for match in pattern.finditer(text):
        for group in match.groups():
            if group:
                add_flag(group)

if config_path.exists():
    import tomllib

    data = tomllib.loads(config_path.read_text())
    parse_sequence(data)

env_flags = os.environ.get("RUSTFLAGS")
if env_flags:
    tokens = shlex.split(env_flags)
    parse_sequence(tokens)

try:
    rg = subprocess.run(
        [
            "rg",
            "--no-heading",
            "--no-line-number",
            "--color=never",
            "-e",
            r"--cfg(?:=|\s+)nexus_[^\s]+",
            str(root),
        ],
        check=False,
        stdout=subprocess.PIPE,
        text=True,
    )
    if rg.stdout:
        parse_text(rg.stdout)
except FileNotFoundError:
    pass

for flag in candidates:
    print(flag)
PY
)

HOST_FLAG=""
OS_FLAG=""

for flag in "${CANDIDATES[@]}"; do
    if [[ -z "${HOST_FLAG}" ]]; then
        if [[ "${flag}" == *'"host"'* ]] || [[ "${flag}" == *'_host'* ]] || [[ "${flag}" == *=host ]]; then
            HOST_FLAG="${flag}"
            [[ -n "${OS_FLAG}" ]] && break
            continue
        fi
    fi
    if [[ -z "${OS_FLAG}" ]]; then
        if [[ "${flag}" == *'"os"'* ]] || [[ "${flag}" == *'_os'* ]] || [[ "${flag}" == *=os ]]; then
            OS_FLAG="${flag}"
            [[ -n "${HOST_FLAG}" ]] && break
            continue
        fi
    fi
    if [[ -n "${HOST_FLAG}" && -n "${OS_FLAG}" ]]; then
        break
    fi
done

HOST_FLAG="${HOST_FLAG:-nexus_host}"
OS_FLAG="${OS_FLAG:-nexus_os}"

printf 'export NEXUS_CFG_HOST=%s\n' "$(printf '%q' "${HOST_FLAG}")"
printf 'export NEXUS_CFG_OS=%s\n' "$(printf '%q' "${OS_FLAG}")"
