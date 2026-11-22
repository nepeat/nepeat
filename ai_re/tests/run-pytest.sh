#!/usr/bin/env bash
# run-pytest.sh WORKERS [pytest arguments...]: pytest for the Justfile recipes,
# in the devshell's python when it is on PATH, else through "nix develop -c".
set -euo pipefail
cd "$(dirname "$0")/.."
n=$1; shift
par=()
if [ "$n" != "1" ]; then par=(-n "$n"); fi
run=(python3 -m pytest -c tests/pytest.ini --rootdir . "${par[@]}" "$@")
if python3 -c 'import pytest, xdist' 2>/dev/null; then
    exec "${run[@]}"
else
    exec nix develop -c "${run[@]}"
fi
