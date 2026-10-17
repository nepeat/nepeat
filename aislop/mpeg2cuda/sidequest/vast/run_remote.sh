#!/usr/bin/env bash
# One-shot on a running instance: upload the bundle (rsync, resumable), run
# bench.sh, pull results, destroy the vast instance (DESTROY=0 keeps it).
#
#   run_remote.sh NAME HOST PORT [INSTANCE_ID [PROXY_HOST PROXY_PORT]]
#
# Tries the direct ssh port and the vast proxy alternately. Logs and results go
# to sidequest/vast/results/<NAME>/. Exit: 0 ok, 2 no ssh (try another host),
# 3 upload failed, 4 bench did not finish. Every failure destroys the instance
# so nothing keeps billing.
#
# The body runs from main(): bash parses the whole file first, so editing this
# script while a run is in flight cannot change what that run executes.
set -uo pipefail

main() {
  local root bundle name host port id phost pport dir
  root="$(cd "$(dirname "$0")/../.." && pwd)"
  bundle="$root/build/vast/bundle"
  name=${1// /_}; host=$2; port=$3; id=${4:-}; phost=${5:-}; pport=${6:-}
  dir="$root/sidequest/vast/results/$name"; mkdir -p "$dir"
  exec > >(tee -a "$dir/run.log") 2>&1

  ssh_opts=(-o BatchMode=yes -o ConnectTimeout=20 -o ServerAliveInterval=15 -o StrictHostKeyChecking=no)
  r() { ssh "${ssh_opts[@]}" -p "$port" "root@$host" "$@" 2>&1 | grep -vE 'Welcome to vast|Have fun|AI agents|vast-agents'; }
  log() { echo "[$(date +%T) $name] $*"; }
  destroy() {
    [[ -n $id && ${DESTROY:-1} == 1 ]] || return 0
    log "destroy $id"; vastai destroy instance "$id" -y 2>&1
  }
  fail() { log "FAIL: $2"; destroy; exit "$1"; }

  # vast installs the key shortly after boot; some hosts only accept it on the
  # proxy, a few never do (fail with 2 so launch.sh can try the next offer)
  log "connect"
  local i err hp conn=0 h=$host p=$port
  for i in $(seq 1 30); do                       # ~8 min
    for hp in "$h:$p" ${phost:+"$phost:$pport"}; do
      if err=$(ssh "${ssh_opts[@]}" -p "${hp##*:}" "root@${hp%:*}" true 2>&1); then
        host=${hp%:*}; port=${hp##*:}; conn=1
        [[ $host == "$phost" ]] && log "using proxy $hp"
        break 2
      fi
    done
    ((i % 4 == 0)) && log "still no ssh: $(grep -vE 'Welcome|Have fun' <<<"$err" | tail -1)"
    sleep 10
  done
  ((conn)) || fail 2 "no ssh after 8 min"
  r 'command -v rsync >/dev/null || { apt-get -qq update && DEBIAN_FRONTEND=noninteractive apt-get -qq install -y rsync >/dev/null; }; command -v rsync' |
    grep -q rsync || fail 3 "could not install rsync"

  log "upload"
  local ok=0
  for i in 1 2 3; do
    rsync -a --mkpath --partial --info=stats1 -e "ssh ${ssh_opts[*]} -p $port" "$bundle/" "root@$host:/root/m2c/bundle/" &&
      { ok=1; break; }
    log "rsync failed, retry $i"; sleep 5
  done
  ((ok)) || fail 3 "upload"

  log "bench"
  r 'cd /root/m2c/bundle && rm -rf results && nohup ./bench.sh < /dev/null > bench.log 2>&1 & sleep 1'
  local last="" cur still=0 fin=0
  for i in $(seq 1 90); do                       # up to ~30 min
    sleep 20
    cur=$(r 'tail -1 /root/m2c/bundle/bench.log')
    [[ $cur == results:* ]] && { fin=1; break; }
    [[ $cur == "$last" ]] && still=$((still + 1)) || still=0
    ((still >= 15)) && { log "no progress for 5 min"; break; }
    last=$cur
  done

  log "fetch"
  rsync -a -e "ssh ${ssh_opts[*]} -p $port" "root@$host:/root/m2c/bundle/bench.log" \
    "root@$host:/root/m2c/bundle/results/" "$dir/"
  grep -E '^(md5|encode|nvdec|e2e),' "$dir/bench.log" 2>/dev/null | tr '\n' ' ' | cut -c1-300; echo
  ((fin)) || fail 4 "bench did not finish (partial results kept)"
  destroy
  log "done"
}

main "$@"; exit
