#!/usr/bin/env python3
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

"""Resolve profile-driven QEMU input capabilities from SystemUI profile TOML."""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path


def profile_manifest_path(repo_root: Path, profile: str) -> Path:
    return (
        repo_root
        / "source"
        / "services"
        / "systemui"
        / "manifests"
        / "profiles"
        / profile
        / "profile.toml"
    )


def load_input_flags(manifest_path: Path) -> dict[str, bool]:
    with manifest_path.open("rb") as handle:
        data = tomllib.load(handle)
    input_cfg = data.get("input")
    if not isinstance(input_cfg, dict):
        raise ValueError("missing [input] section")
    flags: dict[str, bool] = {}
    for key in ("touch", "mouse", "kbd", "remote", "rotary"):
        value = input_cfg.get(key)
        if not isinstance(value, bool):
            raise ValueError(f"invalid input.{key}")
        flags[key] = value
    return flags


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", default=Path(__file__).resolve().parents[1], type=Path)
    parser.add_argument("--profile", required=True)
    args = parser.parse_args()

    manifest_path = profile_manifest_path(args.repo_root, args.profile)
    if not manifest_path.exists():
        print(f"[error] missing SystemUI profile manifest: {manifest_path}", file=sys.stderr)
        return 1
    try:
        flags = load_input_flags(manifest_path)
    except ValueError as exc:
        print(f"[error] invalid SystemUI profile manifest {manifest_path}: {exc}", file=sys.stderr)
        return 1

    print(f"NEXUS_PROFILE_INPUT_TOUCH={int(flags['touch'])}")
    print(f"NEXUS_PROFILE_INPUT_MOUSE={int(flags['mouse'])}")
    print(f"NEXUS_PROFILE_INPUT_KBD={int(flags['kbd'])}")
    print(f"NEXUS_PROFILE_INPUT_REMOTE={int(flags['remote'])}")
    print(f"NEXUS_PROFILE_INPUT_ROTARY={int(flags['rotary'])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
