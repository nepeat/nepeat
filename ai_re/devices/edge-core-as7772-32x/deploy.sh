#!/usr/bin/env bash
# Rebuild the installed AS7772 in place: cross-build on this workstation, copy
# the closure to the switch, activate it, and let the bootloader hook
# (ai_re/nix/modules/boards/as7760/sd-install.nix) refresh /boot/firmware on the SSD.
#   ./deploy.sh            # switch (activate now + make default)
#   ./deploy.sh boot       # make default for next boot only
#   ./deploy.sh dry-activate
#   DEPLOY_REV=<commit> ./deploy.sh [action]   # that committed revision, whatever the working tree holds
#   DEPLOY_ATTR=<name>  ./deploy.sh [action]   # another nixosConfiguration of the flake (default as7772)
# Uses root's SSH key (root@sea420-desktop is authorized on the switch).
set -euo pipefail
host=${AS7772_HOST:-10.60.2.23}
action=${1:-switch}
# The ai_re root flake as a git flake over the whole monorepo (reaches rust/
# for fand). Tracked files only: a new file that is not "git add -N"ed stops
# the deploy here instead of being left out of the build.
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
attr=${DEPLOY_ATTR:-as7772}
if [ -n "${DEPLOY_REV:-}" ]; then
  # A committed revision: the working tree (dirty or with untracked files of other work) is not read.
  rev=$(git -C "$repo" rev-parse --verify "${DEPLOY_REV}^{commit}")
  echo "deploy.sh: revision $rev" >&2
  flake="git+file://$repo?dir=ai_re&rev=$rev#$attr"
else
  "$repo/ai_re/tools/nix-untracked.sh"
  flake="git+file://$repo?dir=ai_re#$attr"
fi
export NIX_SSHOPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10"
exec sudo --preserve-env=NIX_SSHOPTS nixos-rebuild "$action" --flake "$flake" \
  --target-host "root@$host" --no-reexec
