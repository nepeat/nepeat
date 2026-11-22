#!/usr/bin/env bash
# Copy the compiled Evora MDIO script (mkscript.py output, private: vendor
# firmware words) to the switch's private state directory, where
# openbcm-mdio-script.service reads it at boot. See ../sonic.md.
#   ./install-script.sh [SCRIPT]     default: ../dumps/x/evora-trace/evora-100g-bypass.mdio
set -euo pipefail
host=${AS7772_HOST:-10.60.2.23}
here=$(cd "$(dirname "$0")" && pwd)
src=${1:-$here/../dumps/x/evora-trace/evora-100g-bypass.mdio}
dst=/var/lib/evora-phy/evora-100g-bypass.mdio
[ "$(head -c 13 "$src")" = "mdio-script 1" ] || { echo "$src: not an MDIO script" >&2; exit 1; }
sum=$(sha256sum < "$src" | cut -d' ' -f1)
sudo ssh "root@$host" 'install -d -m 0700 /var/lib/evora-phy'
sudo scp -q "$src" "root@$host:$dst.new"
sudo ssh "root@$host" "set -e; echo '$sum  $dst.new' | sha256sum -c - >/dev/null; chmod 0600 $dst.new; mv $dst.new $dst; sync; ls -l $dst"
echo "installed, sha256 $sum"
