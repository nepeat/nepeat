#!/usr/bin/env bash
# 2 h SONiC soak on the cage 1 <-> 16 loop (config-b-vrf-bgp-lag.json): once a
# minute BGP/BFD/LACP state, root-port health and per-daemon RSS; every 10 min
# a flap (alternately Ethernet0 admin down 30 s, PortChannel1 admin down 30 s)
# with the time until both BGP sessions and BFD are back. A ping runs across
# the loop (Vrf-a 10.66.1.1 -> Vrf-b 10.66.2.1, 5/s) the whole time.
export PATH=/run/current-system/sw/bin
VTY=$1; RC="$2"; DUR=${3:-7200}; LOG=/run/soak.txt
END=$(( $(date +%s) + DUR ))
ip vrf exec Vrf-a ping -i 0.2 -W 1 -I 10.66.1.1 10.66.2.1 > /run/soak-ping.txt 2>&1 &
PING=$!
bgp_up() { $VTY -c "show bgp vrf all summary" | awk '/^10\.0\.1\./ && $10 ~ /^[0-9]+$/' | wc -l; }
bfd_up() { $VTY -c "show bfd peers brief" | grep -c " up "; }
rss() { ps -eo comm=,rss= | awk '$2 > 4000 {a[$1]+=$2} END {for (k in a) printf "%s=%d ", k, a[k]}'; }
note() { echo "$(date +%T) $*" >> $LOG; }
note "soak start, $DUR s; $(uptime)"
n=0; flap=0
while [ $(date +%s) -lt $END ]; do
  rp=$(setpci -s 0002:00:00.0 CAP_EXP+0x0a.w CAP_EXP+0x12.w ECAP_AER+0x04.l | tr '\n' ' ')
  dead=$([ -e /run/cs8210-qsfp.dead ] && echo DEAD || echo ok)
  lag=$($RC -n 6 hget "LAG_TABLE|PortChannel1" oper_status)/$($RC -n 6 hget "LAG_TABLE|PortChannel2" oper_status)
  note "bgp=$(bgp_up)/2 bfd=$(bfd_up)/2 lag=$lag rp=[$rp] fpga=$dead load=$(cut -d' ' -f1 /proc/loadavg) rss[KB]: $(rss)"
  if [ $((n % 10)) -eq 5 ]; then
    flap=$((flap + 1))
    if [ $((flap % 2)) -eq 1 ]; then key="PORT|Ethernet0"; else key="PORTCHANNEL|PortChannel1"; fi
    note "flap $flap: $key admin down"
    $RC -n 4 hset "$key" admin_status down >/dev/null
    sleep 30
    note "flap $flap: during: bgp=$(bgp_up)/2 bfd=$(bfd_up)/2; admin up"
    $RC -n 4 hset "$key" admin_status up >/dev/null
    t0=$(date +%s)
    while [ $(( $(date +%s) - t0 )) -lt 180 ]; do
      [ "$(bgp_up)" = 2 ] && [ "$(bfd_up)" = 2 ] && break
      sleep 1
    done
    note "flap $flap: recovered after $(( $(date +%s) - t0 )) s: bgp=$(bgp_up)/2 bfd=$(bfd_up)/2"
  fi
  n=$((n + 1))
  sleep 60
done
kill -INT $PING; sleep 1
note "soak end; ping: $(tail -2 /run/soak-ping.txt | tr '\n' ' ')"
note "bfd counters: $($VTY -c "show bfd peers counters" | grep -E "peer|down events|up events" | tr -s ' \t' ' ' | tr '\n' ';')"
