#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Run the cs8210 SONiC stack (hardware.cs8210.sonic) by hand from a built
# system closure, without switching or deploying a generation: the closure's
# sonic units are linked into /run/systemd/system (gone at reboot), the
# users, sysctls, modules, /etc and tmpfiles bits that the units need are
# added for the run and removed again by "down". Box side, as root.
#   sonic-handrun.sh up TOPLEVEL CONFIG_DB.json   # installs the config as /var/lib/sonic/config_db.json, starts sonic.target
#   sonic-handrun.sh down                          # stops everything and undoes "up"
#   sonic-handrun.sh status
# Refuses unless only cages 1 and 16 hold modules and cs8210-switch is stopped.
set -euo pipefail
export PATH=/run/current-system/sw/bin:$PATH
ST=/run/sonic-handrun
FW_IFS="Ethernet+ PortChannel+ Vlan+ Vrf+"
die() { echo "sonic-handrun: $*" >&2; exit 3; }
units() { (cd "$1/etc/systemd/system" && ls -d redis-sonic.service sonic.target sonic.target.wants sonic-syncd.service.requires cs8210-sai-files.service sonic-*.service); }
case ${1:-} in
up)
  top=$(readlink -f "$2"); cfg=$3
  [ -x "$top/activate" ] || die "$top is not a system closure"
  [ -r "$cfg" ] || die "no config $cfg"
  [ -e $ST/top ] && die "already up ($(cat $ST/top)); down first"
  q=$(cs8210-qsfp --enable 2>&1) || die "cs8210-qsfp failed"
  [[ $q == *"present=00008001 "* ]] || die "cages: not exactly 1 and 16 populated"
  ! systemctl is-active -q cs8210-switch || die "cs8210-switch is active"
  # /etc/systemd/system wins over /run/systemd/system: a generation with
  # SONiC would run its own units, not the closure's.
  [ ! -e /etc/systemd/system/sonic.target ] || die "the running generation has SONiC (hardware.cs8210.sonic); boot one without it"
  ! pgrep -x 'syncd|sai-script|orchagent|qemu-x86_64' >/dev/null || die "an SAI user runs"
  mkdir -p $ST
  echo "$top" > $ST/top
  # netdevs before the run: "down" removes the ones the stack adds (Loopback*, Bridge, dummy)
  ip -o link show | awk -F': ' '{print $2}' | cut -d@ -f1 > $ST/links.before
  # users / groups of the closure's users-groups.json (frr, frrvty, redis-sonic)
  for g in frr frrvty redis-sonic; do getent group $g >/dev/null || { groupadd -r $g; echo $g >> $ST/groups; }; done
  getent passwd frr >/dev/null || { useradd -r -g frr -G frrvty -d /var/empty -s /run/current-system/sw/bin/nologin frr; echo frr >> $ST/users; }
  getent passwd redis-sonic >/dev/null || { useradd -r -g redis-sonic -d /var/empty -s /run/current-system/sw/bin/nologin redis-sonic; echo redis-sonic >> $ST/users; }
  # sysctls and kernel modules of the closure
  for k in net.ipv4.ip_forward net.ipv6.conf.all.forwarding vm.overcommit_memory; do echo "$k=$(sysctl -n $k)" >> $ST/sysctl; done
  sysctl -qw net.ipv4.ip_forward=1 net.ipv6.conf.all.forwarding=1 vm.overcommit_memory=1
  for m in 8021q bridge dummy i2c-dev team team_mode_loadbalance vrf; do modprobe $m; done
  # tmpfiles lines the closure adds (dirs, /run/redis, /usr/share/sonic, /zmq_swss, ...)
  diff /run/current-system/etc/tmpfiles.d/00-nixos.conf "$top/etc/tmpfiles.d/00-nixos.conf" | sed -n 's/^> //p' > $ST/tmpfiles.conf || true
  systemd-tmpfiles --create $ST/tmpfiles.conf
  # /etc entries of the closure that the running system lacks
  for e in sonic frr/vtysh.conf; do
    if [ ! -e /etc/$e ]; then
      mkdir -p "$(dirname /etc/$e)"
      if [ -d "$top/etc/$e" ]; then mkdir /etc/$e; for f in "$top/etc/$e"/*; do ln -s "$(readlink -f "$f")" "/etc/$e/$(basename "$f")"; done
      else ln -s "$(readlink -f "$top/etc/$e")" /etc/$e; fi
      echo /etc/$e >> $ST/etc
    fi
  done
  # firewall: BGP and BFD on the switch interfaces (what the closure's
  # frr/bfd openFirewall do when deployed); a VRF's local traffic reaches
  # INPUT with the VRF device as its interface, hence Vrf+.
  for r in "-p tcp --dport 179" "-p udp --dport 3784" "-p udp --dport 4784" "-p udp --dport 3785" "-p icmp"; do
    for i in $FW_IFS; do iptables -I nixos-fw 1 -i $i $r -j nixos-fw-accept; done
    echo "$r" >> $ST/iptables
  done
  # the host lldpd holds /run/lldpd.socket (the closure disables it for SONiC's)
  if systemctl is-active -q lldpd || systemctl is-failed -q lldpd; then systemctl stop lldpd; systemctl reset-failed lldpd 2>/dev/null || true; echo lldpd > $ST/stopped; fi
  mkdir -p /var/lib/sonic
  if [ -e /var/lib/sonic/config_db.json ]; then cp /var/lib/sonic/config_db.json $ST/config_db.json.before; fi
  install -m 0644 "$cfg" /var/lib/sonic/config_db.json
  for u in $(units "$top"); do ln -sfn "$top/etc/systemd/system/$u" /run/systemd/system/$u; echo $u >> $ST/units; done
  systemctl daemon-reload
  systemctl start sonic.target --no-block
  echo "sonic-handrun: started sonic.target from $top"
  ;;
down)
  [ -e $ST/top ] || die "not up"
  systemctl stop sonic.target || true
  for _ in $(seq 1 60); do pgrep -x 'syncd|orchagent' >/dev/null || break; sleep 1; done
  systemctl stop $(grep -v '\.wants$\|\.requires$' $ST/units) 2>/dev/null || true
  for u in $(cat $ST/units); do rm -f /run/systemd/system/$u; done
  systemctl daemon-reload
  systemctl reset-failed 'sonic-*' redis-sonic.service cs8210-sai-files.service 2>/dev/null || true
  pkill -x teamd || true
  # team devices / VRFs the stack left
  for d in $(ip -o link show type team 2>/dev/null | awk -F': ' '{print $2}' | cut -d@ -f1); do ip link del "$d" || true; done
  for d in $(ip -o link show type vrf 2>/dev/null | awk -F': ' '{print $2}' | cut -d@ -f1); do ip link del "$d" || true; done
  # Loopback*, Bridge and dummy from intfmgrd/vlanmgrd (not there before "up")
  for d in $(ip -o link show | awk -F': ' '{print $2}' | cut -d@ -f1); do
    case $d in Loopback*|Bridge|dummy) grep -qx "$d" $ST/links.before 2>/dev/null || ip link del "$d" || true ;; esac
  done
  # vrfmgrd moved the local rule from pref 0 to 1001; with it only at 1001 a
  # networkd restart deletes it as foreign and the host gets nothing (no SSH;
  # 2026-11-16, sai-hw-sonic-boot-20261116.txt section 2). Put it back.
  for f in -4 -6; do
    if ! ip $f rule show pref 0 | grep -q 'lookup local'; then ip $f rule add pref 0 table local; fi
    while ip $f rule show pref 1001 | grep -q 'lookup local'; do ip $f rule del pref 1001 table local; done
  done
  while read -r r; do
    for i in $FW_IFS; do iptables -D nixos-fw -i $i $r -j nixos-fw-accept 2>/dev/null || true; done
  done < $ST/iptables
  # the host lldpd only after SONiC's has gone (it holds /run/lldpd.socket)
  if [ -e $ST/stopped ]; then
    for _ in $(seq 1 20); do pgrep -x lldpd >/dev/null || break; sleep 0.5; done
    systemctl reset-failed lldpd 2>/dev/null || true
    systemctl start lldpd || true
    sleep 2
    systemctl is-active -q lldpd || { systemctl reset-failed lldpd; systemctl restart lldpd || echo "sonic-handrun: host lldpd did not start" >&2; }
  fi
  while read -r kv; do sysctl -qw "$kv"; done < $ST/sysctl
  if [ -e $ST/etc ]; then while read -r e; do rm -rf "$e"; done < $ST/etc; fi
  if [ -e $ST/users ]; then while read -r u; do userdel "$u" || true; done < $ST/users; fi
  if [ -e $ST/groups ]; then while read -r g; do groupdel "$g" 2>/dev/null || true; done < $ST/groups; fi
  rm -rf /usr/share/sonic /zmq_swss /run/redis /run/sonic-nix /var/warmboot/teamd
  if [ -e $ST/config_db.json.before ]; then cp $ST/config_db.json.before /var/lib/sonic/config_db.json; else rm -rf /var/lib/sonic; fi
  rm -rf $ST
  echo "sonic-handrun: down"
  ;;
status)
  if [ -e $ST/top ]; then echo "up: $(cat $ST/top)"; else echo "not up"; fi
  systemctl list-units --all --no-pager --plain 'sonic-*' redis-sonic.service cs8210-sai-files.service 2>/dev/null | head -60
  ;;
*) echo "usage: $0 up TOPLEVEL CONFIG_DB.json | down | status" >&2; exit 2 ;;
esac
