#!/usr/bin/env bash
# run-python.sh SCRIPT [arguments...]: a tests/ script in the devshell's python
# when it is on PATH, else through "nix develop -c" (as run-pytest.sh).
set -euo pipefail
cd "$(dirname "$0")/.."
if python3 -c 'import pytest' 2>/dev/null; then
    exec python3 "$@"
else
    exec nix develop -c python3 "$@"
fi
