#!/bin/sh
# A stand-in for ssh in the integration tests (tests/cli.rs puts it on PATH as "ssh").
# FAKE_SSH_DIR holds: scenario (one word), state (On|Off), normal (On|Off, the "Normal State";
# Off when absent), password (what askpass must answer); it records argv, env and every CLI
# command (log). The outlet table is the recorded transcript fixtures/nboutlet-off.txt, with the
# State and Normal State columns rewritten for On. -s writes state, -ns writes normal, both kept
# between invocations; scenario stuck accepts -s without changing, ns-stuck the same for -ns.
d="$FAKE_SSH_DIR"
sc=$(cat "$d/scenario")
printf '%s\n' "$*" > "$d/argv"
env > "$d/env"
case "$sc" in
  unreachable) echo "ssh: connect to host 10.0.0.9 port 22: Connection timed out" >&2; exit 255 ;;
  hostkey-changed)
    printf '@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n' >&2
    printf '@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n' >&2
    printf 'The fingerprint for the RSA key sent by the remote host is\nSHA256:FAKEFINGERPRINT0123456789.\n' >&2
    printf 'Host key verification failed.\n' >&2; exit 255 ;;
  hostkey-unknown)
    echo "No RSA host key is known for 10.0.0.9 and you have requested strict checking." >&2
    echo "Host key verification failed." >&2; exit 255 ;;
  session-limit) echo "Connection closed by 10.0.0.9 port 22" >&2; exit 255 ;;
esac
pw=$("$SSH_ASKPASS" "apc@10.0.0.9's password: ") || { echo "askpass failed" >&2; exit 255; }
if [ "$pw" != "$(cat "$d/password")" ]; then
  echo "apc@10.0.0.9: Permission denied (password)." >&2; exit 255
fi
echo "authenticated" >> "$d/log"
[ "$sc" = hang ] && exec sleep 30
cat "$FAKE_SSH_FIXTURES/banner.txt"
table() {
  s='s/x/x/'; ns='s/x/x/'
  [ "$(cat "$d/state")" = On ] &&
    s='s/Unknown              Off                  /Unknown              On                   /'
  [ "$(cat "$d/normal" 2>/dev/null)" = On ] &&
    ns='s/^\(Outlet               Unknown              [A-Za-z]*  *\)Off/\1On /'
  sed -e "$s" -e "$ns" "$FAKE_SSH_FIXTURES/nboutlet-off.txt"
}
while IFS= read -r line; do
  line=$(printf '%s' "$line" | tr -d '\r')
  echo "cmd $line" >> "$d/log"
  case "$line" in
    bye) printf '%s\r\nBye.\r\n' "$line"; echo "Connection to 10.0.0.9 closed." >&2; exit 0 ;;
    nboutlet)
      if [ "$sc" = garbage ]; then printf 'nboutlet\r\nOutlet status: who knows\r\nE000: Success\r\n\r\napc>'
      else table; fi
      continue ;;
    "nboutlet -s On"|"nboutlet -s Off")
      printf '%s\r\n' "$line"
      if [ "$sc" = cli-error ]; then printf 'E102: Parameter Error\r\n'
      else
        [ "$sc" = stuck ] || echo "${line##* }" > "$d/state"
        printf 'E000: Success\r\n'
      fi ;;
    "nboutlet -ns On"|"nboutlet -ns Off")
      printf '%s\r\n' "$line"
      if [ "$sc" = cli-error ]; then printf 'E102: Parameter Error\r\n'
      else
        [ "$sc" = ns-stuck ] || echo "${line##* }" > "$d/normal"
        printf 'E000: Success\r\n'
      fi ;;
    *) printf '%s\r\nE101: Command Not Found\r\n' "$line" ;;
  esac
  printf '\r\napc>'
done
exit 0
