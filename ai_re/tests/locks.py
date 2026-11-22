#!/usr/bin/env python3
"""The two lock files pin the systems' nixpkgs: nix/flake.lock (input nixpkgs,
what "nix build path:./nix#..." and most tests use) and the root flake.lock
(input nixpkgs-systems, what deploy.sh builds). They must name one revision,
or the tests build something else than what is deployed.

    locks.py rev      the revision in nix/flake.lock
    locks.py check    exit 1 unless both lock files name it ("just update" keeps them together)
"""
import json
import sys
from pathlib import Path

AI_RE = Path(__file__).resolve().parents[1]


def locked(lock: Path, name: str) -> dict:
    d = json.loads(lock.read_text())
    return d["nodes"][d["nodes"][d["root"]]["inputs"][name]]["locked"]


def revisions() -> tuple[str, str]:
    return locked(AI_RE / "nix/flake.lock", "nixpkgs")["rev"], locked(AI_RE / "flake.lock", "nixpkgs-systems")["rev"]


def main() -> int:
    nix, root = revisions()
    if sys.argv[1:] == ["rev"]:
        print(nix)
        return 0
    if nix != root:
        print(f"lock files differ: nix/flake.lock nixpkgs {nix}, flake.lock nixpkgs-systems {root} (run: just update)", file=sys.stderr)
        return 1
    print(f"both lock files: nixpkgs {nix}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
