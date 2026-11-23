#!/usr/bin/env bash
# Run ON a switch with services.sonic.macsec (root, login shell): is MKA up on every port that has a MACsec profile,
# by itself, and how long after the stack start did it take? Changes nothing; exit 0 = yes, 1 = no.
#   bash -l macsec-up-check.sh [-t seconds] [-s unit | -s @monotonic-seconds]
#     -t  limit: every port must be secured this many seconds after the start (default 60); the check also waits
#         that long for it (so it can be run right after a restart)
#     -s  what "the start" is: the last start of this unit (default sonic-syncd: every stack start goes through it),
#         or a moment given as seconds on the monotonic clock (/proc/uptime), for a port flap
# Secured = orchagent has an egress and an ingress SA for the port (STATE_DB), made since the start (with -s @...: or
# kept from before, for the ports the flap did not touch; give the port time to lose its SAs first), and the egress
# SCI is the address the netdev has now. By itself = macsecmgrd was started at most once since the start (no restart
# by hand or by systemd). Times are from the journal (monotonic clock): the orchagent line that created the first SA
# of the port's second SC. Prints no key material.
set -u
limit=60; since=sonic-syncd
while getopts t:s: o; do case $o in t) limit=$OPTARG;; s) since=$OPTARG;; *) exit 2;; esac; done
db() { sonic-db-cli "$@"; }
now() { cut -d' ' -f1 /proc/uptime; }
case $since in
  @*) t0=${since#@}; what="the given moment";;
  *) us=$(systemctl show -P InactiveExitTimestampMonotonic "$since"); [ "${us:-0}" != 0 ] || { echo "FAIL: $since was never started"; exit 1; }
     t0=$(awk -v u="$us" 'BEGIN { printf "%.3f", u / 1e6 }'); what="the start of $since";;
esac
ports=$(for k in $(db CONFIG_DB keys 'PORT|*'); do [ -n "$(db CONFIG_DB hget "$k" macsec)" ] && echo "${k#PORT|}"; done | sort -V)
[ -n "$ports" ] || { echo "FAIL: no port has a MACsec profile in CONFIG_DB"; exit 1; }
nports=$(echo $ports | wc -w)
echo "== $(date -Is) $(readlink /run/current-system | cut -c12-23)... up $(now) s; $what: $t0 s; limit $limit s; ports: $(echo $ports)"
secured() {   # port -> 0 when it has both SAs and the egress SCI is the netdev's address
  local p=$1 e i a
  e=$(db STATE_DB keys "MACSEC_EGRESS_SA_TABLE|$p|*" | head -1); i=$(db STATE_DB keys "MACSEC_INGRESS_SA_TABLE|$p|*" | head -1)
  a=$(cat "/sys/class/net/$p/address" 2>/dev/null | tr -d :)
  [ -n "$e" ] && [ -n "$i" ] && [ "$(echo "$e" | cut -d'|' -f3 | cut -c1-12)" = "$a" ]
}
mono() { sed -E 's/^\[ *([0-9.]+)\] [^ ]+ /\1 /' | awk -v t="$t0" '$1 >= t'; }
sa_time() {   # port -> when the first SA of its second SC was created since the start
  journalctl -b -o short-monotonic --no-pager -u sonic-orchagent 2>/dev/null | mono |
    awk -v p="$1" '/createMACsecSA: MACsec SA / { split($(NF-2), k, ":"); if (k[1] == p && !(k[2] in seen)) { seen[k[2]] = 1; if (++n == 2) { print $1; exit } } }'
}
while :; do
  pending=; for p in $ports; do { secured "$p" && { [ -n "$(sa_time "$p")" ] || [ "${since#@}" != "$since" ]; }; } || pending="$pending $p"; done
  [ -z "$pending" ] && break
  awk -v n="$(now)" -v t="$t0" -v l="$limit" 'BEGIN { exit !(n - t > l) }' && break
  sleep 0.5
done
log=$(journalctl -b -o short-monotonic --no-pager -u sonic-macsecmgrd 2>/dev/null | mono)
d() { if [ -n "$1" ]; then awk -v a="$1" -v t="$t0" 'BEGIN { printf "%.1f", a - t }'; else echo -; fi; }
rc=0; worst=0
for p in $ports; do
  t=$(sa_time "$p")
  w=$(echo "$log" | awk -v p="'$p'" '/enableMACsec: The MACsec profile/ && index($0, "port " p " loading success") { print $1 }' | tail -1)
  addr=$(cat "/sys/class/net/$p/address" 2>/dev/null); cfg=$(db CONFIG_DB hget "INTERFACE|$p" mac_addr)
  sci=$(db STATE_DB keys "MACSEC_EGRESS_SA_TABLE|$p|*" | head -1 | cut -d'|' -f3)
  if ! secured "$p"; then res="NOT SECURED"; rc=1
  elif [ -z "$t" ]; then case $since in
      @*) res="secured (SAs from before the moment: its MKA session went on)";;
      *) res="NOT SECURED since the start (its SAs are older)"; rc=1;; esac
  elif awk -v a="$t" -v t="$t0" -v l="$limit" 'BEGIN { exit !(a - t <= l) }'; then res=secured
  else res=LATE; rc=1; fi
  [ -n "$t" ] && worst=$(awk -v a="$t" -v t="$t0" -v w="$worst" 'BEGIN { d = a - t; printf "%.1f", (d > w) ? d : w }')
  echo "$p: $res; MKA session started +$(d "$w") s, secured +$(d "$t") s; netdev $addr${cfg:+ (CONFIG_DB mac_addr $cfg)}, egress SCI ${sci:--}"
done
starts=$(echo "$log" | grep -c -F -e '--- Starting macsecmgrd ---')
sessions=$(echo "$log" | grep -c 'enableMACsec: The MACsec profile .* loading success')
dup=$(journalctl -b -o short-monotonic --no-pager -t wpa_supplicant 2>/dev/null | mono | grep -c -E 'duplicate MAC|duplicated SCI')
echo "macsecmgrd starts since: $starts; MKA sessions started since: $sessions; wpa_supplicant 'duplicate MAC' / 'duplicated SCI' messages since: $dup"
case $since in @*) ;; *) [ "$starts" -le 1 ] || { echo "macsecmgrd was started $starts times since $what: not by itself"; rc=1; };; esac
if [ $rc = 0 ]; then echo "PASS: MKA up on $nports ports without intervention, the slowest $worst s after $what (limit $limit s)"
else
  echo "FAIL: MKA is not up by itself on every port within $limit s of $what"
  journalctl -b -o short-monotonic --no-pager -t wpa_supplicant 2>/dev/null | mono | grep KaY | grep -v -i -E 'sak|key=|cak' | cut -c1-160 | tail -6
fi
set +u
exit $rc
