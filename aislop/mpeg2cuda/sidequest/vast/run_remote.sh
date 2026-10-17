#!/usr/bin/env bash
# One-shot: upload the bundle (rsync, resumable), run bench.sh, pull results,
# optionally destroy the vast instance.
#
#   run_remote.sh NAME HOST PORT [INSTANCE_ID [PROXY_HOST PROXY_PORT]]
#                                                  (DESTROY=0 to keep it)
#   run_remote.sh --all instances.tsv              lines: NAME<TAB>HOST<TAB>PORT<TAB>ID
#
# Logs go to sidequest/vast/results/<NAME>/run.log; results next to it.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
bundle="$root/build/vast/bundle"; out="$root/sidequest/vast/results"

if [[ ${1:-} == --all ]]; then
  pids=()
  while IFS=$'\t' read -r name host port id; do
    [[ -z $name || $name == \#* ]] && continue
    "$0" "$name" "$host" "$port" "$id" & pids+=($!)
  done < "$2"
  rc=0; for p in "${pids[@]}"; do wait "$p" || rc=1; done; exit $rc
fi

name=${1// /_}; host=$2; port=$3; id=${4:-}; phost=${5:-}; pport=${6:-}
dir="$out/$name"; mkdir -p "$dir"; exec > >(tee -a "$dir/run.log") 2>&1
ssh_opts=(-o BatchMode=yes -o ConnectTimeout=20 -o ServerAliveInterval=15 -o StrictHostKeyChecking=no -p "$port")
r() { ssh "${ssh_opts[@]}" "root@$host" "$@" 2>&1 | grep -vE 'Welcome to vast|Have fun|AI agents|vast-agents'; }
log() { echo "[$(date +%T) $name] $*"; }

destroy() {
  [[ -n $id && ${DESTROY:-1} == 1 ]] || return 0
  log "destroy $id"; vastai destroy instance "$id" -y 2>&1
}
ok() { ssh "${ssh_opts[@]}" "root@$host" true >/dev/null 2>&1; }

# vast copies the key in shortly after boot; some hosts never accept it on the
# direct port, so fall back to the ssh proxy, and give up (destroying the
# instance so it stops billing) if neither works
log "connect"
conn=0
for i in $(seq 1 48); do
  ok && { conn=1; break; }
  ((i % 6 == 0)) && log "still no ssh: $(ssh "${ssh_opts[@]}" "root@$host" true 2>&1 | grep -vE 'Welcome|Have fun' | tail -1)"
  if [[ -n $phost ]] && ((i % 3 == 0)); then
    ssh_opts[-1]=$pport; h0=$host; host=$phost
    ok && { conn=1; log "using proxy $phost:$pport"; break; }
    ssh_opts[-1]=$port; host=$h0
  fi
  sleep 10
done
((conn)) || { log "FAIL: no ssh after 8 min"; destroy; exit 1; }
r 'command -v rsync >/dev/null || { apt-get -qq update && DEBIAN_FRONTEND=noninteractive apt-get -qq install -y rsync >/dev/null; }; command -v rsync'

log "upload"
for i in 1 2 3; do
  rsync -a --mkpath --partial --info=stats1 -e "ssh ${ssh_opts[*]}" "$bundle/" "root@$host:/root/m2c/bundle/" && break
  log "rsync failed, retry $i"; sleep 5
  ((i == 3)) && { log "FAIL: upload"; destroy; exit 1; }
done

log "bench"
r 'cd /root/m2c/bundle && rm -rf results && nohup ./bench.sh < /dev/null > bench.log 2>&1 & sleep 1'
last=""; still=0
for i in $(seq 1 90); do       # up to ~30 min
  sleep 20
  cur=$(r 'tail -1 /root/m2c/bundle/bench.log')
  [[ $cur == results:* ]] && break
  [[ $cur == "$last" ]] && still=$((still + 1)) || still=0
  ((still >= 15)) && { log "no progress for 5 min"; break; }
  last=$cur
done

log "fetch"
rsync -a -e "ssh ${ssh_opts[*]}" "root@$host:/root/m2c/bundle/bench.log" "root@$host:/root/m2c/bundle/results/" "$dir/"
grep -E '^(md5|encode|nvdec|e2e),' "$dir/bench.log" | tr '\n' ' ' | cut -c1-400; echo

destroy
log "done"
