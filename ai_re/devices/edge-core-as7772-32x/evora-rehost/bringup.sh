#!/usr/bin/env bash
# Bring Evora PHYs up in 100G MACsec bypass with the re-hosted vendor library
# (see ../evora-rehost.md), from the workstation, PHYs fresh after power-on.
#   ./bringup.sh 0x00 2        # first id, count (default: cages 1-2)
# The ASIC temperature reader is stopped while the MDIO bridge holds the SDK
# and started again at the end (fand goes failsafe 100% meanwhile).
# Then, for traffic: ../asic-link-2dacs.soc through bcm.user on the switch.
# These commands were run by hand (2026-11-21, cages 1-2); this wrapper itself has never been run.
# At boot the switch does all 32 PHYs itself (../sonic.md); this is for a group by hand with SONiC stopped.
# The bridge runs "init soc" + "init misc" (MDIO rate and hold delay from config.bcm); never talk to the PHYs after a bare "init soc".
# Never send clause-22 frames to these PHYs (extphy.md).
set -euo pipefail
first=${1:-0x00}; n=${2:-2}; host=${AS7772_HOST:-10.60.2.23}
here=$(cd "$(dirname "$0")" && pwd); t=$here/../dumps/x/evora-trace; mkdir -p "$t"
tag=bringup-$first-$(date +%Y%m%d-%H%M%S)
sudo ssh "root@$host" systemctl stop openbcm-asic-temp
trap 'sudo ssh "root@$host" systemctl start openbcm-asic-temp' EXIT
python3 "$here/rehost.py" --backend ssh --trace "$t/$tag.trace.txt" --log "$t/$tag.console.txt" \
  --save-state "$t/$tag.state" "init $first $n" \
  "cfg $first nphy=$n speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1" "ver $first" "status $first"
