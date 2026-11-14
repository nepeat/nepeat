#!/usr/bin/env bash
set -euo pipefail
cd /root/papermc
test -x result/bin/papermc
# A separate persistent unit search directory leaves NixOS-owned units alone.
units=/etc/systemd/system.attached
mkdir -p "$units/multi-user.target.wants"
ln -sfn /root/papermc/papermc.service "$units/papermc.service"
ln -sfn ../papermc.service "$units/multi-user.target.wants/papermc.service"
printf 'eula=true\n' > eula.txt
systemctl daemon-reload
systemctl start papermc
