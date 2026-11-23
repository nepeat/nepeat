# trial-boot-verify: runs early on a trial boot (kernel command line trialboot=<nonce>).
# Preamble (from Nix): IFACE HOSTS CHECK_GATEWAY WINDOW INTERVAL KICK_TIMEOUT
# REBOOT_TIMEOUT STATE, wd_kick() wd_arm() wd_disarm().
# Whatever goes wrong in here ends in the revert (EXIT trap), never in a trial
# that stays up unverified.
set +o errexit
verified=0
why="the verifier exited unexpectedly"
nonce=$(sed -n 's/.*\btrialboot=\([0-9a-f]*\).*/\1/p' /proc/cmdline)
log() { echo "trial-boot-verify: $*"; }
record() { echo "$nonce $(date -Is) $*" >> "$STATE/history"; sync "$STATE/history" 2>/dev/null; }
# run by the EXIT trap (so shellcheck's SC2329 is off for this script)
revert() {
  [ "$verified" = 1 ] && return
  echo failed > /run/trial-boot/state
  record "FAILED: $why; rebooting to the default"
  log "FAILED: $why; rebooting to the default"
  wd_arm "$REBOOT_TIMEOUT" || log "watchdog arm failed"
  systemctl reboot
}
trap revert EXIT
mkdir -p /run/trial-boot "$STATE"
echo verifying > /run/trial-boot/state
t0=$(cut -d. -f1 /proc/uptime)
log "trial $nonce up at $t0 s since kernel start; checking $IFACE within $WINDOW s"
wd_kick "$KICK_TIMEOUT" || log "watchdog kick failed"

check() {
  why=""
  [ -e "/sys/class/net/$IFACE" ] || { why="no $IFACE"; return 1; }
  [ "$(cat "/sys/class/net/$IFACE/carrier" 2>/dev/null)" = 1 ] || { why="$IFACE has no carrier"; return 1; }
  ip -4 addr show dev "$IFACE" scope global | grep -q 'inet ' || { why="$IFACE has no IPv4 address"; return 1; }
  if [ "$CHECK_GATEWAY" = 1 ]; then
    local gw; gw=$(ip -4 route show default dev "$IFACE" | awk '{print $3; exit}')
    [ -n "$gw" ] || { why="no default route on $IFACE"; return 1; }
    ping -c1 -W2 -I "$IFACE" "$gw" >/dev/null 2>&1 || { why="gateway $gw does not answer"; return 1; }
  fi
  local h
  for h in $HOSTS; do
    ping -c1 -W2 "$h" >/dev/null 2>&1 || { why="$h does not answer"; return 1; }
  done
}

start=$(date +%s)
while :; do
  if check; then
    wd_disarm || log "watchdog disarm failed"
    up=$(cut -d. -f1 /proc/uptime)
    verified=1
    echo verified > /run/trial-boot/state
    record "verified at $up s (kernel), $(( $(date +%s) - start )) s after the verifier started: $IFACE up, gateway and [$HOSTS] answer; not promoted"
    log "verified at $up s since kernel start; the trial stays up until the next reboot (then the default); 'trial-boot promote' keeps it"
    exit 0
  fi
  if [ $(( $(date +%s) - start )) -ge "$WINDOW" ]; then break; fi
  wd_kick "$KICK_TIMEOUT" || log "watchdog kick failed"
  sleep "$INTERVAL"
done
why="after $WINDOW s: $why"
exit 1
