#!/usr/bin/env bash
# Import a raw firmware blob into a Ghidra project (headless) and auto-analyse.
# Run inside the devshell (or: nix develop <ai_re> -c tools/ghidra/import-raw.sh ...).
#
#   import-raw.sh <binary> <project-dir> <project-name> <base-addr> [language-id]
#
# language-id defaults to AARCH64:LE:64:v8A. List ids with:
#   ls $(dirname $(readlink -f $(command -v ghidra)))/../lib/ghidra/Ghidra/Processors/*/data/languages/*.ldefs
# Existing program of the same name in the project is overwritten.
set -euo pipefail
[ $# -ge 4 ] || { sed -n '2,9p' "$0"; exit 2; }
bin=$(readlink -f "$1"); proj=$(readlink -f -m "$2"); name=$3; base=$4
lang=${5:-AARCH64:LE:64:v8A}
mkdir -p "$proj"
exec ghidra-analyzeHeadless "$proj" "$name" \
  -import "$bin" -overwrite \
  -loader BinaryLoader -loader-baseAddr "$base" \
  -processor "$lang" \
  -analysisTimeoutPerFile 900
