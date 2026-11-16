#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Box side of the oracle (run as root on the CS8210; run-oracle.sh copies it
# there). One SAI script on one SAI, its libifcs calls traced:
#   box-run.sh ours ORACLE_OURS SAI_LIB PROFILE SCRIPT OUTDIR
#   box-run.sh isai ORACLE_ISAI RUNNER  PROFILE SCRIPT OUTDIR
# ours: native sai-script on our libsai, SAI_TERALYNX_IFCS_TRACE = the
#   ours tracer; ipd stays legacy_abi=1.
# isai: isai-qemu sai-script (amd64) with the isai tracer preloaded in the
#   guest; ipd reloaded with legacy_abi=0 for the run and back to 1 after
#   (also on failure).
# Refuses unless only cages 1 and 16 hold modules, ipd is in the expected
# mode, and no SAI user (cs8210-switch, syncd, qemu, sai-*) runs.
# Each run is a transient unit with RuntimeMaxSec; output in OUTDIR:
# trace.txt (tracer + step markers), stdout.txt (the unit's journal).
set -euo pipefail
kind=$1 oracle=$2 dep=$3 profile=$4 script=$5 out=$6
SDK=/var/lib/cs8210-sdk/root/root/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK
: "${ORACLE_LD:=}"   # LD_LIBRARY_PATH for ours (SDK dir + jansson/libyaml/zlib), from run-oracle.sh
die() { echo "box-run: $*" >&2; exit 3; }
q=$(cs8210-qsfp --enable 2>&1) || die "cs8210-qsfp failed"
[[ $q == *"present=00008001 "* ]] || die "cages: not exactly 1 and 16 populated"
! pgrep -x 'syncd|qemu-x86_64|sai-bench|sai-script|orchagent|phy_server' >/dev/null || die "an SAI/SDK user runs"
! systemctl is-active -q cs8210-switch || die "cs8210-switch is active"
[ -r "$profile" ] && [ -r "$script" ] || die "profile/script missing"
mkdir -p "$out"
rm -f "$out/trace.txt"
abi() { cat /sys/module/ipd/parameters/legacy_abi; }
reload() {   # reload ipd with legacy_abi=$1 (Y|N)
  sync
  if lsmod | grep -q '^ipd '; then rmmod ipd; fi
  if [ "$1" = N ]; then modprobe ipd legacy_abi=0; else modprobe ipd; fi
  [ "$(abi)" = "$1" ] || die "ipd legacy_abi is $(abi), wanted $1"
  for _ in 1 2 3 4 5 6 7 8 9 10; do [ -e /dev/ipd ] || [ -e /dev/ipd0 ] && break; sleep 0.5; done
}
u=oracle-$kind-$(date +%H%M%S)
case $kind in
ours)
  [ "$(abi)" = Y ] || die "ipd not in legacy_abi=1"
  grep -v '^SAI_TERALYNX_IFCS_TRACE=' "$profile" > "$out/profile"
  echo "SAI_TERALYNX_IFCS_TRACE=$oracle/lib/libifcs-trace.so" >> "$out/profile"
  profile=$out/profile
  sync
  set +e
  systemd-run --wait --collect --unit="$u" -p RuntimeMaxSec=600 -p WorkingDirectory="$out" \
    --setenv=II_ROOT="$out/ii" --setenv=IFCS_TRACE_FILE="$out/trace.txt" --setenv=ORACLE="$oracle" \
    --setenv=LD_LIBRARY_PATH="$dep/lib:$SDK:$ORACLE_LD" \
    "$oracle/bin/sai-script" "$profile" "$script" >/dev/null 2>&1
  rc=$?
  set -e
  ;;
isai)
  [ "$(abi)" = Y ] || die "ipd not in legacy_abi=1 before the isai run"
  trap 'reload Y; echo "box-run: ipd back to legacy_abi=$(abi)"' EXIT
  reload N
  sync
  set +e
  systemd-run --wait --collect --unit="$u" -p RuntimeMaxSec=900 -p WorkingDirectory="$out" \
    --setenv=HOME="$out" --setenv=PATH=/run/current-system/sw/bin --setenv=IFCS_TRACE_FILE="$out/trace.txt" \
    --setenv=ORACLE="$oracle" \
    --setenv=ISAI_QEMU_PRELOAD="$oracle/lib/libifcs-trace.so" \
    "$dep/bin/isai-qemu" "$oracle/bin/sai-script" "$profile" "$script" >/dev/null 2>&1
  rc=$?
  set -e
  ;;
*) die "kind ours|isai" ;;
esac
journalctl -u "$u" --no-pager -o cat > "$out/stdout.txt" || true
echo "box-run: $kind unit $u rc=$rc, $(grep -c '^T ' "$out/trace.txt" 2>/dev/null || echo 0) IFCS calls traced"
grep -E '^STEP|^RESULT|^EXEC|^DIAG|^trap |^CoPP|sai-script:|script error|event port' "$out/stdout.txt" | tail -n 400 || true
exit $rc
