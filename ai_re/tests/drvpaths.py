#!/usr/bin/env python3
"""Derivation paths of every target, for neutrality proofs of a refactor.

    just drvpaths snapshot          # before the change: writes .drvpaths/base.tsv
    ... edit ...
    just drvpaths diff              # after: lists every target whose drvPath moved

A target whose drvPath is unchanged builds the same thing, bit for bit: that
is the proof ai_re/CLAUDE.md asks for. One that changed needs an explanation
("--explain" runs nix-diff on each pair) and a build.

Targets are the ones "just test" builds (nixtargets.py): every path:./nix
package and system, the EXTRAS, and the root git flake (deploy form). Nothing
is built. Options:
    NAME            snapshot name (default "base"); a path with a "/" is used as is
    -k REGEX        only targets whose id matches (search)
    --no-git        leave out the git: targets (they follow every tracked file
                    of the monorepo, other people's uncommitted edits included)
    --flake REF     evaluate REF instead of path:./nix (e.g. a saved copy of nix/)
    --explain       diff: nix-diff for every changed pair
The file is "id<TAB>drvPath" lines, sorted: diff two of them with diff(1) too.
"""
from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
STORE = HERE.parent / ".drvpaths"   # git-ignored


def snapshot_path(name: str) -> Path:
    return Path(name) if "/" in name else STORE / f"{name}.tsv"


def evaluate(args) -> dict[str, str]:
    if args.flake:
        os.environ["AI_RE_PATH_FLAKE"] = args.flake
    sys.path.insert(0, str(HERE))
    import nixtargets as nt   # after the environment is set: it reads it on import

    d = nt.discover()
    for e in d["errors"]:
        print(f"drvpaths: discovery: {e}", file=sys.stderr)
    targets = [nt.Target(**t) for t in d["targets"]]
    if args.no_git:
        targets = [t for t in targets if not t.id.startswith("git:")]
    if args.k:
        targets = [t for t in targets if re.search(args.k, t.id)]

    def one(t):
        if t.error:
            return t.id, "ERROR discovery"
        r = nt.run(t.eval_cmd(), nt.EVAL_TIMEOUT)
        if r.returncode != 0:
            # Not the message: it carries store paths of the flake copy, which
            # differ between two trees. Known limits are named as such.
            return t.id, "ERROR known-broken" if nt.known_broken(t.id) else "ERROR eval"
        return t.id, r.stdout.strip()

    with ThreadPoolExecutor(args.jobs) as pool:
        return dict(pool.map(one, targets))


def load(path: Path) -> dict[str, str]:
    return dict(line.split("\t", 1) for line in path.read_text().splitlines() if line)


def save(path: Path, paths: dict[str, str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(f"{k}\t{v}\n" for k, v in sorted(paths.items())))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("action", choices=["snapshot", "diff"])
    ap.add_argument("name", nargs="?", default="base")
    ap.add_argument("-k", metavar="REGEX")
    ap.add_argument("--no-git", action="store_true")
    ap.add_argument("--flake", metavar="REF")
    ap.add_argument("--explain", action="store_true")
    ap.add_argument("-j", "--jobs", type=int, default=6)
    args = ap.parse_args()
    path = snapshot_path(args.name)

    if args.action == "snapshot":
        now = evaluate(args)
        save(path, now)
        bad = sorted(k for k, v in now.items() if v == "ERROR eval" or v == "ERROR discovery")
        print(f"drvpaths: {len(now)} targets -> {path}" + (f"; do not evaluate: {' '.join(bad)}" if bad else ""))
        return 0

    if not path.exists():
        print(f"drvpaths: no snapshot {path} (run: just drvpaths snapshot)", file=sys.stderr)
        return 2
    old = load(path)
    now = evaluate(args)
    if args.k or args.no_git:   # compare only what was evaluated this time
        old = {k: v for k, v in old.items() if k in now or
               ((not args.k or re.search(args.k, k)) and not (args.no_git and k.startswith("git:")))}
    changed = sorted(k for k in old.keys() & now.keys() if old[k] != now[k])
    added, removed = sorted(now.keys() - old.keys()), sorted(old.keys() - now.keys())
    for k in changed:
        print(f"CHANGED  {k}\n    - {old[k]}\n    + {now[k]}")
        if args.explain and old[k].startswith("/") and now[k].startswith("/"):
            tool = ["nix-diff"] if shutil.which("nix-diff") else ["nix", "run", "nixpkgs#nix-diff", "--"]
            subprocess.run(tool + ["--color", "never", old[k], now[k]])
    for k in added:
        print(f"ADDED    {k}  {now[k]}")
    for k in removed:
        print(f"REMOVED  {k}  {old[k]}")
    same = len(old.keys() & now.keys()) - len(changed)
    print(f"drvpaths: {same} unchanged, {len(changed)} changed, {len(added)} added, {len(removed)} removed (against {path})")
    return 1 if changed or added or removed else 0


if __name__ == "__main__":
    sys.exit(main())
