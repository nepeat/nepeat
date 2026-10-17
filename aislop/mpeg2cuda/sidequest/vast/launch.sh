#!/usr/bin/env bash
# Rent offers and run the one-shot bench on each, all in parallel.
#
#   launch.sh offers.tsv      lines: NAME<TAB>OFFER_ID   (NAME becomes the results dir)
#
# Each offer: vastai create instance -> wait until running with ssh -> run_remote.sh
# (upload, bench, fetch, destroy). Instances that never boot are destroyed
# after 10 min. Needs the vastai CLI with an API key and the ssh key in the agent.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
IMAGE=${IMAGE:-nvidia/cuda:12.8.1-base-ubuntu24.04}

one() {
  local name=$1 offer=$2 iid j st d px
  iid=$(vastai create instance "$offer" --image "$IMAGE" --disk 15 --ssh --direct \
        --label "m2c-bench-${name// /_}" --raw | python3 -c 'import json,sys; print(json.load(sys.stdin).get("new_contract",""))')
  [[ -n $iid ]] || { echo "[$name] create failed"; return 1; }
  echo "[$(date +%T) $name] instance $iid"
  for k in $(seq 1 60); do
    j=$(vastai show instance "$iid" --raw 2>/dev/null)
    st=$(jq -r '.actual_status // ""' <<<"$j")
    d=$(jq -r '"\(.public_ipaddr // "" | gsub("\\s";"")):\(.ports["22/tcp"][0].HostPort // "")"' <<<"$j")
    px=$(jq -r '"\(.ssh_host // ""):\(.ssh_port // "")"' <<<"$j")
    if [[ $st == running && $d != *: ]]; then
      echo "[$(date +%T) $name] up: direct $d proxy $px"
      "$here/run_remote.sh" "$name" "${d%:*}" "${d##*:}" "$iid" "${px%:*}" "${px##*:}" 2>&1 |
        grep -E '^\[|rsync error'
      return
    fi
    [[ $(jq -r '.status_msg // ""' <<<"$j") == *rror* ]] && break
    sleep 10
  done
  echo "[$(date +%T) $name] never came up: $(jq -r '.status_msg // ""' <<<"$j" | head -c 120)"
  vastai destroy instance "$iid" -y
}

while IFS=$'\t' read -r name offer; do
  [[ -z $name || $name == \#* ]] && continue
  one "$name" "$offer" &
done < "$1"
wait
