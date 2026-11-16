#!/usr/bin/env bash
# Deploy the DMC-12 NixOS system in place (profile-rebuildable), the same steps nixos-rebuild
# --target-host takes: build here from the ai_re ROOT flake (a git flake over the monorepo, so
# fand is in; `path:./nix` can't reach ../rust and would fall back), copy only new paths with
# root's ssh key (the box trusts root@<workstation>), set the system profile, activate.
#   ./deploy.sh 10.60.2.10            # switch
#   ./deploy.sh 10.60.2.10 boot       # kernel/DTB changes: then reboot the box
#   ./deploy.sh 10.60.2.10 test       # activate without a boot entry
# NIX_BUILD_LOCK=<file>: build under flock (one build at a time on a shared builder).
set -euo pipefail
ip=${1:?usage: deploy.sh IP [switch|boot|test]}
action=${2:-switch}
case $action in switch|boot|test) ;; *) echo "deploy.sh: action is switch, boot or test" >&2; exit 2 ;; esac
attr=${ATTR:-dmc12}
here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
# A git flake sees tracked files only: an untracked module would silently be missing.
# Scope: what the DMC-12 system is built from (other devices' work in progress is not our business).
untracked=$(git -C "$repo" ls-files --others --exclude-standard -- ai_re/flake.nix ai_re/nix/default.nix \
  ai_re/nix/configs ai_re/nix/modules/profiles ai_re/nix/modules/platforms ai_re/nix/modules/boards/dmc12 \
  ai_re/nix/modules/thermal ai_re/nix/modules/monitoring ai_re/nix/packages/linux-alpine \
  ai_re/nix/packages/al-eth ai_re/nix/packages/dmc12-metrics ai_re/nix/packages/dmc12-cpld-reset \
  ai_re/nix/packages/fastfetch-tiny rust/consoled)
if [ -n "$untracked" ]; then
  echo "deploy.sh: untracked files the flake would not see (git add -N them):" >&2
  echo "$untracked" >&2
  exit 1
fi
# Build from the committed HEAD only: a git flake would otherwise include uncommitted edits
# of tracked files, i.e. whatever another session has in progress (e.g. fand). Our own paths
# must be committed first.
dirty=$(git -C "$repo" status --porcelain --untracked-files=no -- ai_re/flake.nix ai_re/nix/default.nix \
  ai_re/nix/configs ai_re/nix/modules/profiles ai_re/nix/modules/platforms ai_re/nix/modules/boards/dmc12 \
  ai_re/nix/modules/thermal ai_re/nix/modules/monitoring ai_re/nix/packages/linux-alpine \
  ai_re/nix/packages/al-eth ai_re/nix/packages/dmc12-metrics ai_re/nix/packages/dmc12-cpld-reset \
  ai_re/nix/packages/fastfetch-tiny rust/consoled)
others=$(git -C "$repo" status --porcelain --untracked-files=no -- rust/crates/fand)
if [ -n "$others" ]; then
  echo "deploy.sh: note: fand has uncommitted edits (another session's work); building the committed fand" >&2
fi
if [ -n "$dirty" ]; then
  echo "deploy.sh: uncommitted changes in paths this system is built from (commit them, or wait for their owner):" >&2
  echo "$dirty" >&2
  exit 1
fi
rev=$(git -C "$repo" rev-parse HEAD)
flake="git+file://$repo?dir=ai_re&rev=$rev"
build=(nix build --no-link --print-out-paths "$flake#nixosConfigurations.$attr.config.system.build.toplevel")
if [ -n "${NIX_BUILD_LOCK:-}" ]; then out=$(flock "$NIX_BUILD_LOCK" "${build[@]}"); else out=$("${build[@]}"); fi
opts=(-o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)
ssh=(sudo -n ssh "${opts[@]}" "root@$ip")
written() { "${ssh[@]}" 'awk "\$3==\"sda\"{print \$10}" /proc/diskstats'; }
before=$(written)
sudo -n env NIX_SSHOPTS="${opts[*]}" nix-copy-closure --to "root@$ip" "$out"
if [ "$action" != test ]; then
  "${ssh[@]}" nix-env -p /nix/var/nix/profiles/system --set "$out"
fi
"${ssh[@]}" "$out/bin/switch-to-configuration" "$action"
after=$(written)
echo "deploy.sh: $action $out (rev ${rev:0:8}) on $ip; SSD writes during deploy: $(( (after - before) / 2 )) KiB"
if [ "$action" = boot ]; then echo "deploy.sh: reboot the box to use it"; fi
