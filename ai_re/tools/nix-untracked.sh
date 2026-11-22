#!/usr/bin/env bash
# nix-untracked.sh [PATH...]: for the deploy.sh scripts. They build the root
# flake as a git flake, which contains tracked files only: a new file that is
# not "git add"ed is missing from the build (an import of it fails with "path
# does not exist"; a file found by a directory listing is silently left out).
# Lists such files under the paths (default: everything the systems are built
# from) and exits 1 if there are any. DEPLOY_ALLOW_UNTRACKED=1: list, exit 0.
set -euo pipefail
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
[ $# -gt 0 ] || set -- ai_re/flake.nix ai_re/flake.lock ai_re/nix rust/crates/fand rust/consoled
untracked=$(git -C "$repo" ls-files --others --exclude-standard -- "$@")
[ -n "$untracked" ] || exit 0
echo "untracked files the git flake does not see (git add -N them; DEPLOY_ALLOW_UNTRACKED=1 deploys without them):" >&2
echo "$untracked" | sed 's/^/  /' >&2
[ -n "${DEPLOY_ALLOW_UNTRACKED:-}" ]
