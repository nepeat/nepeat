# shellcheck shell=bash disable=SC2034  # (variables for the scripts that source this file)
# Sourced by sonic-traffic-test.sh, macsec-traffic-test.sh, macsec-{rekey,wrongcak,wrap,clear-window}-test.sh and
# loadtest/sonic-ring.sh (run ON the AS7772, root).
# - Before the first write: a snapshot of the whole CONFIG_DB and of the kernel state these scripts touch, in
#   $ST_RUN/<name> (/run: a reboot regenerates CONFIG_DB from Nix and drops it).
# - Restore of the touched CONFIG_DB keys to the snapshot on every exit path (EXIT/INT/TERM/HUP traps; a second ^C,
#   a closed ssh or a dead tee do not cut it short), then a check that every CONFIG_DB key, field and value and that
#   kernel state equal the snapshot: a difference is printed and fails the run.
# - A run refuses to start over the leftovers of an earlier one, naming the key; --cleanup removes them;
#   --dry-run prints the writes and makes none.
# Writes go straight to CONFIG_DB (what the 'config' CLI writes, without its 4 s start), each followed by a wait for
# the manager's STATE_DB entry (intfmgrd / vrfmgrd / vlanmgrd write one when they have applied a key) and for
# orchagent to have taken the APPL_DB entry.
# Environment, for the offline test (loadtest/test/run.sh): ST_REDIS, ST_RUN, ST_WAIT (s per wait), ST_SETTLE (s).
# `bash -l script`: the exit builtin of a login shell reads the logout files before the EXIT trap, under the script's
# set -u, and NixOS /etc/bash_logout reads an unset variable: every exit status became 1. Exit without nounset:
exit() { local rc=$?; set +u; builtin exit "${1:-$rc}"; }
: "${ST_REDIS:=redis-cli -s /var/run/redis/redis.sock}"
: "${ST_RUN:=/run/as7772-tests}"
: "${ST_WAIT:=20}"
: "${ST_SETTLE:=2}"
ST_DRY=0 ST_MODE=run ST_ARGS=() ST_TMPDIR=0 ST_NAME='' ST_DIR='' ST_PHASES=() ST_FORCED=0 ST_REFUSED=0 ST_BG=() ST_MKA_S=0
read -r -a ST__R <<< "$ST_REDIS"
CDB=4 APPL=0 STATE=6   # redis database numbers of CONFIG_DB, APPL_DB, STATE_DB (database_config.json)
P4="Ethernet0 Ethernet4 Ethernet120 Ethernet124"
# the far ends' own MAC addresses (board base + 1, + 2; nix/configs/as7772 services.sonic.settings.INTERFACE)
FAR_MAC_Ethernet4=1c:ea:0b:38:2b:d6 FAR_MAC_Ethernet124=1c:ea:0b:38:2b:d7
TAB=$'\t'

st_args() {   # [--dry-run] [--cleanup] [args...]
  local a
  for a in "$@"; do
    case $a in
      --dry-run|-n) ST_DRY=1 ;;
      --cleanup) ST_MODE=cleanup ;;
      *) ST_ARGS+=("$a") ;;
    esac
  done
  [ "$ST_DRY" = 1 ] && echo "== DRY RUN: the writes are printed, none is made"
  return 0
}
st_r() { local db=$1; shift; "${ST__R[@]}" -n "$db" --raw "$@" < /dev/null; }
st_has() { [ "$(st_r "$1" exists "$2")" = 1 ]; }
st_hget() { st_r "$CDB" hget "$1" "$2"; }

# whole database -> sorted "key<TAB>field<TAB>value" lines (a string key: field "="; another type: field "<type>")
st_dump() {
  local db=$1 k t
  st_r "$db" --scan --pattern '*' | LC_ALL=C sort -u | while IFS= read -r k; do
    [ -n "$k" ] || continue
    t=$(st_r "$db" type "$k")
    case $t in
      hash) st_r "$db" hgetall "$k" | paste -d '\t' - - | awk -v k="$k" '{ print k "\t" $0 }' ;;
      string) printf '%s\t=\t%s\n' "$k" "$(st_r "$db" get "$k")" ;;
      none) ;;
      *) printf '%s\t<%s>\t\n' "$k" "$t" ;;
    esac
  done | LC_ALL=C sort
}
# usable only when redis answered and SONiC's loader had run: an empty snapshot would make the restore delete keys
st_dump_ok() { grep -q "^CONFIG_DB_INITIALIZED$TAB" "$1"; }

st__secret() { [[ ${1,,} =~ (^|_)(cak|sak|auth_key|salt)$ ]]; }   # a field whose value is never printed
st__mask() { awk -F'\t' 'BEGIN { OFS = FS } tolower($2) ~ /(^|_)(cak|sak|auth_key|salt)$/ { $3 = "<not shown>" } 1'; }
st_w() {   # a redis write: printed (key material masked), made unless dry-run
  local db=$1 a m=0; shift
  printf '  redis-cli -n %s' "$db"
  for a in "$@"; do
    if [ "$m" = 1 ]; then printf ' <not shown>'; m=0; else printf ' %q' "$a"; st__secret "$a" && m=1; fi
  done; echo
  [ "$ST_DRY" = 1 ] && return 0
  st_r "$db" "$@" > /dev/null
}
st_x() {   # a kernel / host write: printed, made unless dry-run
  printf '  run'; printf ' %q' "$@"; echo
  [ "$ST_DRY" = 1 ] && return 0
  "$@"
}
st_wait() {   # description, command...: poll for up to ST_WAIT s
  local what=$1 n; shift
  [ "$ST_DRY" = 1 ] && { echo "  (would wait: $what)"; return 0; }
  for ((n = 0; n < ST_WAIT * 4; n++)); do "$@" && return 0; sleep 0.25; done
  echo "TIMEOUT after $ST_WAIT s: $what"; return 1
}
st_state_key() {   # CONFIG_DB key -> the STATE_DB entry its manager writes once it has applied it
  case $1 in
    INTERFACE\|*) echo "INTERFACE_TABLE|${1#INTERFACE|}" ;;
    VRF\|*) echo "VRF_TABLE|${1#VRF|}" ;;
    VLAN\|*) echo "VLAN_TABLE|${1#VLAN|}" ;;
    VLAN_MEMBER\|*) echo "VLAN_MEMBER_TABLE|${1#VLAN_MEMBER|}" ;;
  esac
}
st_appl_key() {   # ... and the APPL_DB entry orchagent has taken (a ProducerStateTable key appears / goes when popped)
  local s; s=$(st_state_key "$1"); [ -n "$s" ] || return 0
  s=${s/#INTERFACE_TABLE|/INTF_TABLE|}; echo "${s//|/:}"
}
st_absent() { ! st_has "$1" "$2"; }
st_wait_applied() {   # key present|absent: the manager (STATE_DB), then orchagent (APPL_DB)
  local s a; s=$(st_state_key "$1") a=$(st_appl_key "$1"); [ -n "$s" ] || return 0
  if [ "$2" = present ]; then
    st_wait "STATE_DB $s (applied $1)" st_has "$STATE" "$s" && st_wait "APPL_DB $a (orchagent took $1)" st_has "$APPL" "$a"
  else
    st_wait "STATE_DB $s gone (removed $1)" st_absent "$STATE" "$s" && st_wait "APPL_DB $a gone (orchagent took the delete of $1)" st_absent "$APPL" "$a"
  fi
}

# plan to bring the keys matching an ERE back to the snapshot: "1 D key" (delete), "2 S key f v" / "2 H key f" (set /
# delete a field in place), "3 S key f v" (re-create after the delete). A changed vrf_name is a delete + re-create:
# intfmgrd does not move a router interface between VRFs in place ("can not change to ... directly").
st__plan() {
  RE=$3 awk -F'\t' '
    function fields(list, f) { return split(substr(list, 2), f, "\t") }
    BEGIN { re = ENVIRON["RE"] }   # not -v: awk would turn a backslash escape in it into something else
    FNR == NR { if ($1 ~ re) { bk[$1] = 1; bv[$1, $2] = $3; bf[$1] = bf[$1] "\t" $2 } next }
    $1 ~ re { ck[$1] = 1; cv[$1, $2] = $3; cf[$1] = cf[$1] "\t" $2 }
    END {
      for (k in ck) if (!(k in bk)) print "1\tD\t" k
      for (k in bk) {
        n = fields(bf[k], f); rebind = 0
        if (k in ck) {
          hb = ((k, "vrf_name") in bv); hc = ((k, "vrf_name") in cv)
          rebind = (hb != hc) || (hb && bv[k, "vrf_name"] != cv[k, "vrf_name"])
        }
        if (!(k in ck) || rebind) {
          if (rebind) print "1\tD\t" k
          for (i = 1; i <= n; i++) print "3\tS\t" k "\t" f[i] "\t" bv[k, f[i]]
          continue
        }
        for (i = 1; i <= n; i++) if (!((k, f[i]) in cv) || cv[k, f[i]] != bv[k, f[i]]) print "2\tS\t" k "\t" f[i] "\t" bv[k, f[i]]
        m = fields(cf[k], g)
        for (i = 1; i <= m; i++) if (!((k, g[i]) in bv)) print "2\tH\t" k "\t" g[i]
      }
    }' "$1" "$2" | LC_ALL=C sort -t "$TAB" -k1,1 -k3,3 -s
}
# CONFIG_DB keys back to a snapshot, one ERE per phase, phases in the order given (dependants first); per phase:
# deletes, wait until their managers dropped them, then sets, wait until applied, then ST_SETTLE s for orchagent
st_restore_configdb() {
  local before=$1 re now plan rc=0 op k f v last
  local -a args
  shift
  st_dump_ok "$before" || { echo "REFUSED: $before is not a valid CONFIG_DB snapshot: nothing restored"; return 1; }
  now=$(mktemp) plan=$(mktemp)
  for re in "$@"; do
    st_dump "$CDB" > "$now"
    st__plan "$before" "$now" "$re" > "$plan"
    [ -s "$plan" ] || continue
    echo "  -- phase /$re/: $(wc -l < "$plan") change(s)"
    while IFS="$TAB" read -r _ op k _; do [ "$op" = D ] && st_w "$CDB" del "$k"; done < "$plan"
    while IFS="$TAB" read -r _ op k _; do [ "$op" = D ] && { st_wait_applied "$k" absent || rc=1; }; done < "$plan"
    last='' args=()
    while IFS="$TAB" read -r _ op k f v; do   # one HSET per key, with all its fields
      if [ "$op" = S ] && [ "$k" = "$last" ]; then args+=("$f" "$v"); continue; fi
      [ -n "$last" ] && st_w "$CDB" hset "$last" "${args[@]}"
      last='' args=()
      case $op in S) last=$k args=("$f" "$v") ;; H) st_w "$CDB" hdel "$k" "$f" ;; esac
    done < <(grep -v "^1$TAB" "$plan")
    [ -n "$last" ] && st_w "$CDB" hset "$last" "${args[@]}"
    while IFS= read -r k; do st_wait_applied "$k" present < /dev/null || rc=1; done < <(awk -F'\t' '$1 == 3 { print $3 }' "$plan" | sort -u)
    [ "$ST_DRY" = 1 ] || sleep "$ST_SETTLE"
  done
  rm -f "$now" "$plan"
  return $rc
}

# kernel state of the four DAC ports (master: VRF or bridge; IPv4 addresses), the VRF and VLAN netdevs, and what
# st_kernel_extra prints (a function a script may define)
st_kernel_state() {
  local p
  for p in $P4; do
    printf 'port %s master=%s inet=%s\n' "$p" \
      "$(ip -o link show dev "$p" 2> /dev/null | grep -o 'master [^ ]*' | cut -d' ' -f2)" \
      "$(ip -o -4 addr show dev "$p" 2> /dev/null | awk '{ print $4 }' | LC_ALL=C sort | tr '\n' ',')"
  done
  echo "vrf $(ip -o link show type vrf 2> /dev/null | awk -F': ' '{ print $2 }' | LC_ALL=C sort | tr '\n' ' ')"
  echo "vlan $(ip -o link show type vlan 2> /dev/null | awk -F': ' '{ print $2 }' | cut -d@ -f1 | LC_ALL=C sort | tr '\n' ' ')"
  if declare -F st_kernel_extra > /dev/null; then st_kernel_extra; fi
}
st_kernel_same() { diff -q "$1" <(st_kernel_state) > /dev/null; }
# wait for the managers to undo their kernel changes; what they leave is removed here, named (FORCED), and fails the run
st_restore_kernel() {
  local before=$1 now p a v b c rc=0
  if declare -F st_kernel_restore_extra > /dev/null; then st_kernel_restore_extra "$before"; fi
  [ "$ST_DRY" = 1 ] && { echo "  (would wait for the kernel state of the snapshot)"; return 0; }
  st_wait "kernel state as in the snapshot" st_kernel_same "$before" && return 0
  now=$(st_kernel_state)
  for p in $P4; do
    b=$(grep "^port $p " "$before") c=$(grep "^port $p " <<< "$now")
    [ "$b" = "$c" ] && continue
    if [[ $b == *" master= "* && $c != *" master= "* ]]; then
      echo "FORCED: $p still had a master the managers did not remove"; st_x ip link set dev "$p" nomaster; rc=1
    fi
    for a in $(tr ',' ' ' <<< "${c#*inet=}"); do
      [[ ,${b#*inet=} == *",$a,"* ]] || { echo "FORCED: $p still had $a"; st_x ip addr del "$a" dev "$p"; rc=1; }
    done
  done
  # shellcheck disable=SC2013  # names, one word each
  for v in $(grep '^vrf ' <<< "$now" | cut -c5-); do
    grep '^vrf ' "$before" | grep -q -w -- "$v" || { echo "FORCED: VRF netdev $v was left"; st_x ip link del dev "$v"; rc=1; }
  done
  # shellcheck disable=SC2013
  for v in $(grep '^vlan ' <<< "$now" | cut -c6-); do
    grep '^vlan ' "$before" | grep -q -w -- "$v" || { echo "FORCED: VLAN netdev $v was left"; st_x ip link del dev "$v"; rc=1; }
  done
  [ "$rc" = 0 ] || ST_FORCED=1
  return 0
}

st_verify() {   # the whole CONFIG_DB and the kernel state against the snapshot
  local d=$ST_DIR rc=0
  [ "$ST_DRY" = 1 ] && { echo "== dry-run: nothing written, nothing verified"; return 0; }
  st_dump "$CDB" > "$d/configdb-after.tsv"
  if diff -u "$d/configdb-before.tsv" "$d/configdb-after.tsv" > "$d/configdb.diff"; then
    echo "== CONFIG_DB identical to the snapshot: $(cut -f1 "$d/configdb-after.tsv" | sort -u | wc -l) keys, $(wc -l < "$d/configdb-after.tsv") fields"
  else
    echo "== FAIL: CONFIG_DB DIFFERS from the snapshot ($d/configdb.diff; - snapshot, + now):"
    grep -E '^[-+][^-+]' "$d/configdb.diff" | st__mask; rc=1
  fi
  st_kernel_state > "$d/kernel-after.txt"
  if diff -u "$d/kernel-before.txt" "$d/kernel-after.txt" > "$d/kernel.diff"; then echo "== kernel state identical to the snapshot"
  else echo "== FAIL: kernel state DIFFERS from the snapshot (- snapshot, + now):"; grep -E '^[-+][^-+]' "$d/kernel.diff"; rc=1; fi
  if declare -F st_verify_extra > /dev/null; then st_verify_extra || rc=1; fi
  return $rc
}
# st_restore_pre_extra (a script may define it): before the CONFIG_DB restore (hardware a test took by hand);
# st_restore_extra: after it (a daemon that reads its config only at start)
st_restore() {
  local rc=0
  if declare -F st_restore_pre_extra > /dev/null; then st_restore_pre_extra || rc=1; fi
  st_restore_configdb "$ST_DIR/configdb-before.tsv" "${ST_PHASES[@]}" || return 1
  if declare -F st_restore_extra > /dev/null; then st_restore_extra || rc=1; fi
  st_restore_kernel "$ST_DIR/kernel-before.txt" || rc=1
  return $rc
}

st_mkdir() {   # the snapshot directory (mode 700: CONFIG_DB holds the MACsec profiles' keys); a temporary one in a dry run
  if [ "$ST_DRY" = 1 ]; then ST_DIR=$(mktemp -d) ST_TMPDIR=1; else (umask 077 && mkdir -p "$ST_DIR") || exit 2; fi
}
st_begin() {   # name [arm]: snapshot into $ST_RUN/<name>; with arm the restore runs on every exit of this shell
  ST_NAME=$1 ST_DIR=$ST_RUN/$1
  st_mkdir
  st_dump "$CDB" > "$ST_DIR/configdb-before.tsv.new"
  if ! st_dump_ok "$ST_DIR/configdb-before.tsv.new"; then
    echo "ABORT: no CONFIG_DB snapshot (redis not answering, or no CONFIG_DB_INITIALIZED): nothing written"
    rm -f "$ST_DIR/configdb-before.tsv.new"; rmdir "$ST_DIR" 2> /dev/null; exit 2
  fi
  st_kernel_state > "$ST_DIR/kernel-before.txt"
  if declare -F st_snapshot_extra > /dev/null; then st_snapshot_extra; fi
  mv "$ST_DIR/configdb-before.tsv.new" "$ST_DIR/configdb-before.tsv"
  echo "== snapshot: $ST_DIR ($(cut -f1 "$ST_DIR/configdb-before.tsv" | sort -u | wc -l) CONFIG_DB keys)"
  if [ "${2:-}" = arm ]; then st_arm; fi
}
st_arm() { trap st__on_exit EXIT; trap 'exit 130' INT; trap 'exit 143' TERM; trap 'exit 129' HUP; }
st_disarm() { trap - EXIT INT TERM HUP; }
st__on_exit() {   # [status]
  local rc=${1:-$?}
  trap '' INT TERM HUP PIPE
  trap - EXIT
  if [ "${#ST_BG[@]}" -gt 0 ]; then   # the script's background jobs (ST_BG+=($!)): stopped before the restore
    kill "${ST_BG[@]}" 2> /dev/null; wait "${ST_BG[@]}" 2> /dev/null; ST_BG=()
  fi
  echo "== restore ($ST_NAME; exit status so far $rc)"
  if st_restore && st_verify; then
    if [ "$ST_FORCED" = 1 ]; then
      echo "== RESTORED and verified, but only with the FORCED kernel removals above: a SONiC manager did not undo its change"
      [ "$rc" = 0 ] && rc=5
    elif [ "$ST_DRY" = 0 ]; then echo "== RESTORED and verified"; fi
    if [ "$ST_DRY" = 0 ] || [ "$ST_TMPDIR" = 1 ]; then rm -rf "$ST_DIR"; fi
  else
    echo "== RESTORE NOT VERIFIED (the FAIL / TIMEOUT lines above): the snapshot stays in $ST_DIR; fix the cause, then --cleanup (or reboot)"
    [ "$ST_TMPDIR" = 1 ] && rm -rf "$ST_DIR"
    [ "$rc" = 0 ] && rc=4
  fi
  exit "$rc"
}
st_refuse() { echo "REFUSED: $*"; ST_REFUSED=$((ST_REFUSED + 1)); }
st_other_runs() {   # names the caller checks itself: any other unfinished run in $ST_RUN (its snapshot) is refused
  local f n
  for f in "$ST_RUN"/*/configdb-before.tsv; do
    [ -e "$f" ] || continue
    n=${f%/configdb-before.tsv} n=${n##*/}
    case " $* " in *" $n "*) continue ;; esac
    st_refuse "$ST_RUN/$n exists: $n.sh did not finish its restore ($n.sh --cleanup)"
  done
}
st_refusals() {   # end of a check: 0 when nothing was refused
  if [ "$ST_REFUSED" = 0 ]; then return 0; fi
  if [ "$ST_DRY" = 1 ]; then echo "(dry run: a real run would stop here: $ST_REFUSED refusal(s))"; return 0; fi
  echo "== $ST_REFUSED refusal(s): nothing was changed"; return 1
}

# --cleanup: the snapshot of an interrupted run if there is one, else st_synth_configdb / st_synth_kernel (the
# script's model of the clean state, made from the current one by dropping what this test, or its older versions
# without a snapshot, leave behind)
st_cleanup() {   # name
  ST_NAME=$1 ST_DIR=$ST_RUN/$1
  if [ -s "$ST_DIR/configdb-before.tsv" ]; then
    echo "== cleanup: restoring the snapshot of an interrupted run ($ST_DIR, taken $(date -r "$ST_DIR/configdb-before.tsv" -Is))"
    if [ "$ST_DRY" = 1 ]; then
      local real=$ST_DIR; st_mkdir; cp "$real"/* "$ST_DIR"/
    fi
  else
    echo "== cleanup: no snapshot in $ST_DIR: restoring the clean state this script knows"
    st_mkdir
    st_dump "$CDB" | st_synth_configdb > "$ST_DIR/configdb-before.tsv"
    st_kernel_state | st_synth_kernel > "$ST_DIR/kernel-before.txt"
    if declare -F st_synth_extra > /dev/null; then st_synth_extra; fi
  fi
  st_dump_ok "$ST_DIR/configdb-before.tsv" || { echo "ABORT: no valid CONFIG_DB dump"; exit 2; }
  st__on_exit 0
}

# ---- the VRF-pair topology of the two traffic tests: cages 1 <-> 2 and 31 <-> 32, each far end in its own VRF
ST_PAIRS="Ethernet0=10.0.1.0/31 Ethernet4=10.0.1.1/31:Vrf-a Ethernet120=10.0.31.0/31 Ethernet124=10.0.31.1/31:Vrf-b"
ST_PAIRS_PHASES=('^INTERFACE[|]Ethernet(0|4|120|124)[|]' '^INTERFACE[|]Ethernet(0|4|120|124)$' '^VRF[|]Vrf-[ab]$')

st_pairs_check() {   # refuses on any trace of an earlier run of this topology or of the load ring
  local k p v
  ST_REFUSED=0
  [ -e "$ST_RUN/vrf-pairs/configdb-before.tsv" ] && st_refuse "$ST_RUN/vrf-pairs exists: an earlier traffic test did not finish its restore (--cleanup)"
  [ -e "$ST_RUN/sonic-ring/configdb-before.tsv" ] && st_refuse "$ST_RUN/sonic-ring exists: the load ring is up or was interrupted (loadtest/sonic-ring.sh down)"
  st_other_runs vrf-pairs sonic-ring
  for k in 'VRF|Vrf-a' 'VRF|Vrf-b'; do st_has "$CDB" "$k" && st_refuse "CONFIG_DB $k exists (--cleanup)"; done
  for p in $P4; do
    v=$(st_hget "INTERFACE|$p" vrf_name); [ -n "$v" ] && st_refuse "CONFIG_DB INTERFACE|$p has vrf_name=$v (--cleanup)"
    for k in $(st_r "$CDB" --scan --pattern "INTERFACE|$p|*"); do st_refuse "CONFIG_DB $k exists (--cleanup)"; done
    for k in $(st_r "$CDB" --scan --pattern "VLAN_MEMBER|*|$p"); do st_refuse "CONFIG_DB $k exists: $p is a VLAN member (the load ring? loadtest/sonic-ring.sh down)"; done
  done
  while IFS= read -r k; do st_refuse "kernel: $k (--cleanup)"; done < <(st_kernel_state | grep -E '^vrf .*Vrf-[ab]|^port .* master=[^ ]|^port .* inet=[^ ]')
  return 0
}
st_pairs_up() {   # the writes of 'config vrf add', 'config interface vrf bind', 'config interface ip add'
  local e p a v f x
  for v in Vrf-a Vrf-b; do st_w "$CDB" hset "VRF|$v" NULL NULL; done
  for v in Vrf-a Vrf-b; do st_wait_applied "VRF|$v" present || return 1; done
  for e in $ST_PAIRS; do
    p=${e%%=*} a=${e#*=} v=
    [[ $a == *:* ]] || continue
    v=${a#*:}
    # a router interface does not change VRF in place: the entry goes, then comes back with vrf_name + its fields
    local -a keep=()
    while IFS="$TAB" read -r f x; do [ -n "$f" ] && keep+=("$f" "$x"); done < <(st_r "$CDB" hgetall "INTERFACE|$p" | paste -d '\t' - -)
    if st_has "$CDB" "INTERFACE|$p"; then st_w "$CDB" del "INTERFACE|$p"; st_wait_applied "INTERFACE|$p" absent || return 1; fi
    st_w "$CDB" hset "INTERFACE|$p" vrf_name "$v" "${keep[@]}"
    st_wait_applied "INTERFACE|$p" present || return 1
    st_wait "$p in $v (kernel)" st_kernel_master "$p" "$v" || return 1
  done
  for e in $ST_PAIRS; do
    p=${e%%=*} a=${e#*=}; a=${a%%:*}
    if ! st_has "$CDB" "INTERFACE|$p"; then st_w "$CDB" hset "INTERFACE|$p" NULL NULL; st_wait_applied "INTERFACE|$p" present || return 1; fi
    st_w "$CDB" hset "INTERFACE|$p|$a" NULL NULL
  done
  for e in $ST_PAIRS; do
    p=${e%%=*} a=${e#*=}; a=${a%%:*}
    st_wait_applied "INTERFACE|$p|$a" present || return 1
    st_wait "$a on $p (kernel)" st_kernel_addr "$p" "$a" || return 1
  done
}
st_kernel_master() { ip -o link show dev "$1" 2> /dev/null | grep -q "master $2 "; }
st_kernel_addr() { ip -o -4 addr show dev "$1" 2> /dev/null | grep -q -F " $2 "; }
# the clean state for --cleanup without a snapshot: no Vrf-a/b, no INTERFACE entries of Ethernet0/120 nor addresses
# on the four ports, the far ends' INTERFACE entries with exactly their mac_addr (as NixOS generates them)
st_pairs_synth_configdb() {
  awk -F'\t' -v m4="$FAR_MAC_Ethernet4" -v m124="$FAR_MAC_Ethernet124" '
    $1 ~ /^VRF[|]Vrf-[ab]$/ || $1 ~ /^INTERFACE[|]Ethernet(0|4|120|124)([|]|$)/ { next }
    { print }
    END { print "INTERFACE|Ethernet4\tmac_addr\t" m4; print "INTERFACE|Ethernet124\tmac_addr\t" m124 }' | LC_ALL=C sort
}
st_synth_configdb() { st_pairs_synth_configdb; }   # a script with more to undo redefines it, starting from that
st_synth_kernel() { sed -E -e 's/^(port [^ ]+) master=[^ ]* inet=.*/\1 master= inet=/' -e '/^vrf /s/\bVrf-[ab] //g'; }

# ---- MACsec tests on that topology (macsec-rekey / -wrongcak / -wrap / -clear-window-test.sh; MKA must come up by itself)
st_mka() {   # seconds: wait until the PHYs have 8 SAs open (4 ports, egress + ingress); ST_MKA_S = seconds it took
  for ((ST_MKA_S = 0; ST_MKA_S < $1; ST_MKA_S++)); do
    [ "$(openbcm-diag extmacsec | grep -c ' open ')" = 8 ] && return 0
    sleep 1
  done
  return 1
}
st_mka_verify() {   # a MACsec test's st_verify_extra: MKA again on all four ports after the restore
  if st_mka 60; then echo "== MKA: 8 SAs open $ST_MKA_S s after the restore"
  else echo "== FAIL: MKA not up on all four ports 60 s after the restore"; openbcm-diag extmacsec; return 1; fi
}
st_pairs_macsec_check() {   # st_pairs_check, and the far ends have their own MAC (the KaY: "duplicate MAC detected")
  local p want have
  st_pairs_check
  for p in Ethernet4 Ethernet124; do
    want=FAR_MAC_$p have=$(st_hget "INTERFACE|$p" mac_addr)
    [ "$have" = "${!want}" ] || st_refuse "CONFIG_DB INTERFACE|$p mac_addr is '$have', not ${!want}: MACsec needs the far end's own address (--cleanup puts it back)"
  done
  return 0
}
st_pairs_session() {   # name: snapshot, restore armed, the topology up; then (not in a dry run) MKA on all four, ARP
  st_begin "$1" arm
  echo "== setup: VRFs, far ends into them, addresses (CONFIG_DB)"
  st_pairs_up || { echo "FAIL: setup did not complete"; exit 1; }
  [ "$ST_DRY" = 1 ] && return 0
  st_mka 60 || { echo "FAIL: MKA is not up on all four ports 60 s after the setup (nothing is restarted here)"; openbcm-diag extmacsec; exit 1; }
  ping -c 3 -W 2 10.0.1.1 > /dev/null; ping -c 3 -W 2 10.0.31.1 > /dev/null   # ARP
  return 0
}
