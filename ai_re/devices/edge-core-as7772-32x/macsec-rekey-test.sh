#!/usr/bin/env bash
# Run ON the AS7772 (root, login shell), after macsec-traffic-test.sh: SAK changes under traffic.
#   bash -l macsec-rekey-test.sh [period_s] [seconds]
# Sets MACSEC_PROFILE rekey_period at run time (macsecmgrd restarted to take it), floods pings both ways over
# cage 1 <-> 2 (1 ms apart) for the given time, lists every SA change with its time, then puts the declared
# profile back (sonic-macsec-config) and restarts macsecmgrd. No key is printed.
set -u
period=${1:-20}; secs=${2:-100}
export SONIC_CLI_IFACE_MODE=alias
D() { openbcm-diag "$@"; }
prof=$(sonic-db-cli CONFIG_DB hget 'PORT|Ethernet0' macsec)
echo "== $(date -Is) profile $prof: rekey_period $period s, $secs s of pings (1 ms apart, both directions, cage 1 <-> 2)"
sonic-db-cli CONFIG_DB hset "MACSEC_PROFILE|$prof" rekey_period "$period" >/dev/null
systemctl restart sonic-macsecmgrd
for i in $(seq 60); do [ "$(D extmacsec | grep -c ' open ')" = 8 ] && break; sleep 1; done
echo "MKA up after $i s"; sleep 2
ping -c 3 -W 1 10.0.1.1 >/dev/null
t0=$(date +%s.%N); since=$(date '+%Y-%m-%d %H:%M:%S')
ping -q -c $((secs * 1000)) -i 0.001 -W 1 10.0.1.1 > /tmp/rekey-a.txt 2>&1 &
ip vrf exec Vrf-a ping -q -c $((secs * 1000)) -i 0.001 -W 1 10.0.1.0 > /tmp/rekey-b.txt 2>&1 &
ping -q -c $((secs * 100)) -i 0.01 -W 1 10.0.31.1 > /tmp/rekey-c.txt 2>&1 &
wait
echo "-- cage 1 -> 2"; tail -2 /tmp/rekey-a.txt
echo "-- cage 2 -> 1"; tail -2 /tmp/rekey-b.txt
echo "-- cage 31 -> 32 (10 ms apart)"; tail -2 /tmp/rekey-c.txt
echo "-- SA changes in the PHYs (syncd)"
journalctl -u sonic-syncd --since "$since" --no-pager -o short-precise | grep -E 'SA AN [0-9] (installed|removed)' | sed -E 's/^[A-Za-z]+ [0-9]+ ([0-9:.]+) .*api=43: /\1 /' 
echo "-- rekeys per port: $(journalctl -u sonic-syncd --since "$since" --no-pager | grep -c 'egress SA AN [0-9] installed') egress SA installs on 4 ports"
D extmacsec
show macsec Ethernet1/1 2>&1 | grep -E 'encoding_an|SA \(|PKTS_OK|NOT_VALID|INVALID |LATE|UNUSED|NOT_USING|OUT_PKTS_ENCRYPTED|CURRENT_XPN'
journalctl -u sonic-macsecmgrd -u sonic-orchagent -u sonic-syncd --since "$since" --no-pager | grep -i -E ' ERR|WARN|fail' | grep -v -i 'watchdog' | cut -c1-200 | tail -8
echo "== restore: the declared profile, macsecmgrd restarted"
systemctl restart sonic-macsec-config; sonic-db-cli CONFIG_DB hget "MACSEC_PROFILE|$prof" rekey_period
systemctl restart sonic-macsecmgrd
for i in $(seq 60); do [ "$(D extmacsec | grep -c ' open ')" = 8 ] && break; sleep 1; done
echo "MKA up after $i s; failed units: $(systemctl --failed --no-legend | wc -l)"
