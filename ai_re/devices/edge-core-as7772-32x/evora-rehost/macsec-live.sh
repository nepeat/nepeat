#!/usr/bin/env bash
# Staged MACsec test on cages 1 <-> 2 (PHY ids 0x00, 0x01; ASIC ce0, ce1) through the re-hosted vendor library.
# Plan and expected results: ../macsec.md "Live test plan". Run from the workstation (ai_re devshell), one stage
# at a time, and read each stage's output before the next. Run 2026-11-22: all stages passed (../macsec.md).
#   ./macsec-live.sh prep | s0 | s1 | s2 | s3 | s4 | s5 | b | restore
# Rules that apply (../extphy.md): MDIO only after init misc (the bridge does init soc + init misc, the stages
# then run init all), clause 45 only, RAM firmware only (rehost.py refuses the EEPROM path), no NOR/CPLD writes.
# A PHY that stops answering (every register one constant) needs a cold power cycle: lab-power 1 off / on.
set -euo pipefail
host=${AS7772_HOST:-10.60.2.23}
here=$(cd "$(dirname "$0")" && pwd); t=$here/../dumps/x/evora-trace
ts=$(date +%Y%m%d-%H%M%S)
S() { sudo ssh "root@$host" "$@"; }
# one macsec.py session = one bcm.user on the switch (PHY and ASIC commands in order); state chains stage to stage
M() {  # M <stage> <state-in> <state-out> CMD...
  local st=$1 in=$2 out=$3; shift 3
  local load=(--load-state "$in"); [ "$in" = - ] && load=()   # "-" = fresh library: the vendor init refuses a PHY it already knows (rc -24)
  PYTHONUNBUFFERED=1 python3 "$here/macsec.py" --backend ssh "${load[@]}" --save-state "$out" \
    --trace "$t/macsec-$ts-$st.trace.txt" --bridge-log "$t/macsec-$ts-$st.bridge.txt" "$@" 2>&1 | tee "$t/macsec-$ts-$st.console.txt"
}
keys=$t/macsec-test-keys.env      # throw-away test keys, git-ignored with the rest of dumps/
[ -f "$keys" ] || { umask 077; printf 'K1=%s\nK2=%s\nK3=%s\n' "$(openssl rand -hex 16)" "$(openssl rand -hex 16)" "$(openssl rand -hex 32)" > "$keys"; }
. "$keys"
SCI1=1cea0b382bd50001; SCI2=1cea0b382bd50002     # one SCI per transmitter (base MAC + port number); test values
CFG1='extphy cfg 0x00 nphy=1 speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1'
CFG2='extphy cfg 0x01 nphy=1 speed=100000 if_line=CAUI4_C2C if_sys=CAUI fec=1'
# ASIC side: asic-link-2dacs.soc with the vendor's MACsec IPG (48) on the pair under test; ce30/ce31 stay bypass
# (IPG 16) as the control pair. CPU in VLAN 101/102 so a frame with an unknown DA is also flooded to the CPU (pw).
ASIC=( 'diag init all' 'diag vlan remove 1 pbm=ce,xe' 'diag port ce,xe en=0'
       'diag vlan create 101 pbm=ce0 ubm=ce0' 'diag pvlan set ce0 101' 'diag vlan create 102 pbm=ce1 ubm=ce1' 'diag pvlan set ce1 102'
       'diag vlan create 103 pbm=ce30 ubm=ce30' 'diag pvlan set ce30 103' 'diag vlan create 104 pbm=ce31 ubm=ce31' 'diag pvlan set ce31 104'
       'diag port ce0,ce1,ce30,ce31 an=off if=CAUI sp=100000 en=1'
       'diag m CLMAC_TX_CTRL.ce0 AVERAGE_IPG=48' 'diag m CLMAC_TX_CTRL.ce1 AVERAGE_IPG=48'
       'diag m CLMAC_TX_CTRL.ce30 AVERAGE_IPG=16' 'diag m CLMAC_TX_CTRL.ce31 AVERAGE_IPG=16'
       'diag sleep 6' 'diag ps ce0,ce1,ce30,ce31' )
# traffic: 1000 x 256 B ce0 -> ce1, 2000 x 1024 B ce1 -> ce0, control 3000 x 512 B ce30 -> ce31; then every counter
TRAFFIC=( 'extphy reg64 clearmibc 0x00' 'extphy reg64 clearmibc 0x00 if_side=1' 'extphy reg64 clearmibc 0x01' 'extphy reg64 clearmibc 0x01 if_side=1' 'diag clear c' 'diag tx 1000 pbm=ce0 vlan=101 length=256' 'diag sleep 2' 'diag tx 2000 pbm=ce1 vlan=102 length=1024' 'diag sleep 2'
          'diag tx 3000 pbm=ce30 vlan=103 length=512' 'diag sleep 2' 'diag show c ce0,ce1,ce30,ce31' 'diag show c rfcs' 'diag show c rerpkt'
          'mib 0x00' 'mib 0x01' )
case ${1:-} in
prep)   # switch booted into SONiC (PHYs in bypass from the boot script); stop SONiC and the temperature reader
  S 'systemctl stop sonic.target; sleep 12; systemctl stop openbcm-asic-temp; systemctl is-active sonic-syncd openbcm-asic-temp; ls -la /tmp/phy/ 2>&1'
  S 'mkdir -p /tmp/phy; ln -sf $(systemctl cat openbcm-mdio-script | grep -o "/nix/store/[^ ]*config.bcm" | head -1) /tmp/phy/config.bcm; ls -l /tmp/phy'   # the bridge's bcm.user reads ./config.bcm
  ;;
s0)     # baseline: both PHYs bypass, library state = the recorded bypass init. Expect: all frames arrive, no MACsec.
  M s0 "$t/all-pre.state" "$t/macsec-live.s0.state" 'extphy status 0x00' 'bypass? 0x00' 'bypass? 0x01' \
    'reg 0x00 0x4600fffc 0x4400fffc 0x4500fffc 0x4300fffc' "${ASIC[@]}" "${TRAFFIC[@]}"
  ;;
s1)     # vendor mode 2 on cage 1 ONLY (firmware re-download to 0x00, RAM), cage 2 stays bypass = the wire tap:
        # ce0 -> ce1 must arrive 32 B longer per frame (SecTAG + ICV) and unreadable; ce1 -> ce0 must be dropped by 0x00.
  T0=(); for c in "${TRAFFIC[@]}"; do case $c in *0x01*) ;; *) T0+=("$c");; esac; done   # 0x01 is not in this library state yet
  M s1 - "$t/macsec-live.s1.state" 'extphy init 0x00 1 mode=2' "$CFG1" 'extphy status 0x00' 'adopt 0x00' \
    "${ASIC[@]}" "${T0[@]}" 'stats 0x00'
  ;;
s2)     # vendor mode 2 on cage 2 as well (the vendor's fixed test key on both): traffic passes again, original length
  M s2 "$t/macsec-live.s1.state" "$t/macsec-live.s2.state" 'extphy init 0x01 1 mode=2' "$CFG2" 'extphy status 0x01' 'adopt 0x01' \
    "${ASIC[@]}" "${TRAFFIC[@]}" 'stats 0x00' 'stats 0x01' 'extphy reg64 chkecc 0x00' 'extphy reg64 chkmiberr 0x00'
  ;;
s3)     # our keys: replace the vendor SA on both PHYs (K1 for 0x00 -> 0x01, K2 for 0x01 -> 0x00, AN 1)
  M s3 "$t/macsec-live.s2.state" "$t/macsec-live.s3.state" \
    'sa-del 0x00' 'sa-del 0x01' \
    "sa-add 0x00 side=egress key=$K1 sci=$SCI1 an=1"  "sa-add 0x01 side=ingress key=$K1 sci=$SCI1 an=1" \
    "sa-add 0x01 side=egress key=$K2 sci=$SCI2 an=1"  "sa-add 0x00 side=ingress key=$K2 sci=$SCI2 an=1" \
    "${ASIC[@]}" "${TRAFFIC[@]}" 'stats 0x00' 'stats 0x01'
  ;;
s4)     # negative: wrong key on the receiver of ce0 -> ce1 only. Expect: that direction 0 frames at ce1 and
        # in_pkts_not_valid (or invalid) = 1000 at 0x01; the other direction and the control pair unaffected.
        # Then the right key again: the direction recovers.
  M s4 "$t/macsec-live.s3.state" "$t/macsec-live.s4.state" \
    'sa-del 0x01 side=ingress' "sa-add 0x01 side=ingress key=$K2 sci=$SCI1 an=1" \
    "${ASIC[@]}" "${TRAFFIC[@]}" 'stats 0x00' 'stats 0x01' \
    'sa-del 0x01 side=ingress' "sa-add 0x01 side=ingress key=$K1 sci=$SCI1 an=1" "${TRAFFIC[@]}" 'stats 0x01 side=ingress'
  ;;
s5)     # optional: GCM-AES-XPN-256 + replay window on ce0 -> ce1, AN 2 added on the receiver beside AN 1 first (rekey order)
  M s5 "$t/macsec-live.s4.state" "$t/macsec-live.s5.state" \
    "sa-add 0x01 side=ingress key=$K3 sci=$SCI1 an=2 salt=e630e81a48de86a21c66fa6d ssci=7a30c118 window=128 replay=1" \
    'sa-del 0x00 side=egress' "sa-add 0x00 side=egress key=$K3 sci=$SCI1 an=2 salt=e630e81a48de86a21c66fa6d ssci=7a30c118" \
    "${ASIC[@]}" "${TRAFFIC[@]}" 'stats 0x00' 'stats 0x01'
  ;;
b)      # control pair 31 <-> 32 from the booted bypass state, no firmware download: bypass off + our SAs, one end
        # first (the other end is then the tap), then both. These commands passed 2026-11-22, run from a hand-written wrapper.
  B1=1cea0b382bd5001f; B2=1cea0b382bd50020
  BA=( 'diag init all' 'diag vlan remove 1 pbm=ce,xe' 'diag port ce,xe en=0'
       'diag vlan create 103 pbm=ce30 ubm=ce30' 'diag pvlan set ce30 103' 'diag vlan create 104 pbm=ce31 ubm=ce31' 'diag pvlan set ce31 104'
       'diag port ce30,ce31 an=off if=CAUI sp=100000 en=1' 'diag m CLMAC_TX_CTRL.ce30 AVERAGE_IPG=48' 'diag m CLMAC_TX_CTRL.ce31 AVERAGE_IPG=48'
       'diag sleep 6' 'diag ps ce30,ce31' )
  BT=( 'extphy reg64 clearmibc 0x17e' 'extphy reg64 clearmibc 0x17e if_side=1' 'extphy reg64 clearmibc 0x17f' 'extphy reg64 clearmibc 0x17f if_side=1'
       'diag clear c' 'diag tx 3000 pbm=ce30 vlan=103 length=512' 'diag sleep 2' 'diag tx 1500 pbm=ce31 vlan=104 length=300' 'diag sleep 2'
       'diag show c ce30,ce31' 'mib 0x17e' 'mib 0x17f' 'stats 0x17e' 'stats 0x17f' )
  M b "$t/all-pre.state" "$t/macsec-live.B.state" 'extphy status 0x17e' 'bypass? 0x17e' \
    'bypass 0x17e 0' "on 0x17e side=egress key=$K1 sci=$B1 an=1" "on 0x17e side=ingress key=$K2 sci=$B2 an=1" 'bypass? 0x17e' 'extphy status 0x17e' \
    "${BA[@]}" "${BT[@]}" \
    'bypass 0x17f 0' "on 0x17f side=egress key=$K2 sci=$B2 an=1" "on 0x17f side=ingress key=$K1 sci=$B1 an=1" 'extphy status 0x17f' \
    'diag sleep 3' 'diag ps ce30,ce31' "${BT[@]}"
  ;;
restore) # back to the booted SONiC state: a reboot re-runs the bypass script on all 32 PHYs
  S 'systemctl reboot' || true
  echo 'wait 3 min, then on the switch: bash -l sonic-traffic-test.sh (../sonic.md); expect 4 ports up, 0 lost'
  ;;
*) sed -n '2,9p' "$0"; exit 1;;
esac
