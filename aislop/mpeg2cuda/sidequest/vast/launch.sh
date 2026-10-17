#!/usr/bin/env bash
# Rent offers and run the one-shot bench on each, all in parallel.
#
#   launch.sh offers.tsv      lines: NAME<TAB>OFFER_ID[,OFFER_ID...]
#
# pick.py writes this file. For each line: rent the first offer, attach the
# account ssh key, wait until it runs, then run_remote.sh (upload, bench,
# fetch, destroy). If the host never boots or never accepts ssh, the instance
# is destroyed and the next offer on the line is tried. Needs the vastai CLI
# with an API key and the ssh key in the agent.
set -uo pipefail

main() {
  local here
  here="$(cd "$(dirname "$0")" && pwd)"
  IMAGE=${IMAGE:-nvidia/cuda:12.8.1-base-ubuntu24.04}
  # the key vast knows for this account; attaching it explicitly fixes hosts
  # that otherwise answer "Permission denied (publickey)"
  KEY=$(vastai show ssh-keys --raw 2>/dev/null | jq -r '.[0].public_key // empty')

  while IFS=$'\t' read -r name offers; do
    [[ -z $name || $name == \#* ]] && continue
    line "$name" "$offers" &
  done < "$1"
  wait
}

line() {
  local name=$1 offer rc
  for offer in ${2//,/ }; do
    one "$name" "$offer"; rc=$?
    ((rc == 0)) && return 0
    ((rc == 2)) || return "$rc"                  # only boot/ssh failures retry
    echo "[$(date +%T) $name] offer $offer unusable, trying the next one"
  done
  echo "[$(date +%T) $name] FAIL: no usable offer"; return 1
}

one() {
  local name=$1 offer=$2 iid j st d px
  iid=$(vastai create instance "$offer" --image "$IMAGE" --disk 15 --ssh --direct \
        --label "m2c-bench-${name// /_}" --raw | jq -r '.new_contract // empty')
  [[ -n $iid ]] || { echo "[$name] create $offer failed"; return 2; }
  echo "[$(date +%T) $name] offer $offer -> instance $iid"
  [[ -n $KEY ]] && vastai attach ssh "$iid" "$KEY" >/dev/null 2>&1
  for _ in $(seq 1 60); do                       # 10 min to boot
    j=$(vastai show instance "$iid" --raw 2>/dev/null)
    st=$(jq -r '.actual_status // ""' <<<"$j")
    d=$(jq -r '"\(.public_ipaddr // "" | gsub("\\s";"")):\(.ports["22/tcp"][0].HostPort // "")"' <<<"$j")
    px=$(jq -r '"\(.ssh_host // ""):\(.ssh_port // "")"' <<<"$j")
    if [[ $st == running && $d != *: ]]; then
      echo "[$(date +%T) $name] up: direct $d proxy $px"
      "$here/run_remote.sh" "$name" "${d%:*}" "${d##*:}" "$iid" "${px%:*}" "${px##*:}" |
        grep -E '^\[|rsync error'
      return "${PIPESTATUS[0]}"
    fi
    [[ $(jq -r '.status_msg // ""' <<<"$j") == *rror* ]] && break
    sleep 10
  done
  echo "[$(date +%T) $name] never came up: $(jq -r '.status_msg // ""' <<<"$j" | head -c 120)"
  vastai destroy instance "$iid" -y >/dev/null
  return 2
}

main "$@"; exit
