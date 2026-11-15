#!/usr/bin/env bash
# Rebuild the installed AS7760 in place: cross-build on this workstation, copy
# the closure to the switch's /nix/store, activate it, and let the bootloader
# hook (ai_re/nix/modules/boards/as7760/sd-install.nix) refresh /boot/firmware (old generation -> prev/).
#   ./deploy.sh            # switch (activate now + make default)
#   ./deploy.sh boot       # make default for next boot only
#   ./deploy.sh dry-activate
# Uses root's SSH key (root@sea420-desktop is authorized on the switch).
set -euo pipefail
host=${AS7760_HOST:-10.60.2.253}
action=${1:-switch}
# The ai_re root flake, as a git flake over the whole monorepo: it can reach
# rust/ (fand). It sees tracked files only (uncommitted edits to them count);
# "git add -N" new files before deploying.
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
flake="git+file://$repo?dir=ai_re#as7760"
export NIX_SSHOPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10"
exec sudo --preserve-env=NIX_SSHOPTS nixos-rebuild "$action" --flake "$flake" \
  --target-host "root@$host" --no-reexec
