#!/usr/bin/env bash
# Step 0: run Credo's x86_64 SAI against a fake, logging Owl 800 slice,
# sandboxed: no network, no $HOME, no /sys, clean environment, private /tmp,
# empty working directory, hard timeout. Never touches hardware.
#
# usage: run-step0.sh OUTDIR [TIMEOUT_S] [REG VAL ...]
#   env: PROFILE=xml  BOOT_TYPE=cold|fast|warm  FAKE_DIR=dir (seeds+mbox)
#        MBOX_EXTRA=file  STATE_DIR=dir  MERGE=1  FAKE_FW_LOAD_TYPE=skip|force|auto
#        FAKE_QUERY=1 FAKE_MACSEC=1 FAKE_WARM_SHUTDOWN=1 FAKE_GATE=1 MAX_ACCESS=n
#   needs the unpacked debs in ../analysis/research/credo-sai-1.2.12/x
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
X=$HERE/../analysis/research/credo-sai-1.2.12/x
OUT=${1:?outdir}; shift
TMO=${1:-60}; shift || true
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)

SAI=$X/libsaicredo_1.2.12_amd64/usr
OWL=$X/libsaicredo-owl_1.2.12_amd64/usr
# libSerdes is dlopen'ed from CREDO_DEVICE_PATH: give it a dir with only owl800.
mkdir -p "$OUT/serdes" "$OUT/lib"
cp "$OWL/lib/libSerdes_owl800.so" "$OUT/serdes/"
cp "$SAI/lib/libsai.so.1" "$SAI/lib/libcredo.so" "$SAI/lib/libcrlua.so" \
   "$SAI/lib/libdriver_164_u.so" "$OWL/lib/libowl.so" "$OUT/lib/"

CC_LIB=$(nix build --no-link --print-out-paths nixpkgs#stdenv.cc.cc.lib)
gcc -O1 -g -Wall -I"$SAI/include" -o "$OUT/fakephy" "$HERE/fakephy.c" -ldl
# Switch profile: PROFILE=file.xml (default owl800-slice.xml), seen as /credo/profile.xml
cp "${PROFILE:-$HERE/owl800-slice.xml}" "$OUT/profile.xml"
# Optional replay data from a vendor trace (trace2fake.py): FAKE_DIR=dir
FAKE_DIR=${FAKE_DIR:-}
fake=()
if [[ -n $FAKE_DIR ]]; then
  cp "$FAKE_DIR/seeds.txt" "$FAKE_DIR/mbox.txt" "$OUT/"
  fake=(--ro-bind "$OUT/seeds.txt" /credo/seeds.txt --ro-bind "$OUT/mbox.txt" /credo/mbox.txt
        --setenv FAKE_SEEDS /credo/seeds.txt --setenv FAKE_MBOX /credo/mbox.txt)
  if [[ -f $FAKE_DIR/tcm.txt ]]; then
    cp "$FAKE_DIR/tcm.txt" "$OUT/"
    fake+=(--ro-bind "$OUT/tcm.txt" /credo/tcm.txt --setenv FAKE_TCM /credo/tcm.txt)
  fi
fi

# Optional mailbox answers not seen in the trace (labelled, see mbox-extra.txt)
if [[ -n ${MBOX_EXTRA:-} ]]; then
  cp "$MBOX_EXTRA" "$OUT/mbox-extra.txt"
  fake+=(--ro-bind "$OUT/mbox-extra.txt" /credo/mbox-extra.txt --setenv FAKE_MBOX_EXTRA /credo/mbox-extra.txt)
fi
# Writable warm-boot state dir (SAI_WARM_BOOT_READ/WRITE_FILE = /state/warm.bin):
# STATE_DIR=dir (default $OUT/state). Copy a previous run's state in to warm boot from it.
STATE=${STATE_DIR:-$OUT/state}
mkdir -p "$STATE"
# Stage switches for fakephy (see fakephy.c): FAKE_FW_LOAD_TYPE, FAKE_QUERY,
# FAKE_MACSEC, FAKE_WARM_SHUTDOWN
stage=()
for v in FAKE_FW_LOAD_TYPE FAKE_QUERY FAKE_MACSEC FAKE_WARM_SHUTDOWN FAKE_GATE MAX_ACCESS; do
  [[ -n ${!v:-} ]] && stage+=(--setenv "$v" "${!v}")
done

# MERGE=1: libsai's log goes into access.txt too, interleaved with the accesses
ERRF=$OUT/stderr.txt
merge=()
[[ -n ${MERGE:-} ]] && { ERRF=/dev/stdout; merge=(--setenv LOG_STDERR 1); }
set +e
timeout -k 5 "$TMO" bwrap \
  --unshare-all --die-with-parent --new-session --clearenv \
  --ro-bind /nix/store /nix/store \
  --ro-bind "$OUT/lib" /credo/lib \
  --ro-bind "$OUT/serdes" /credo/serdes \
  --ro-bind "$OUT/fakephy" /credo/fakephy \
  --ro-bind "$OUT/profile.xml" /credo/profile.xml \
  --dev /dev --proc /proc --tmpfs /tmp --dir /work --chdir /work \
  --bind "$STATE" /state \
  --setenv PROFILE_SAI_WARM_BOOT_READ_FILE /state/warm.bin \
  --setenv PROFILE_SAI_WARM_BOOT_WRITE_FILE /state/warm.bin \
  --setenv LD_LIBRARY_PATH "/credo/lib:$CC_LIB/lib" \
  --setenv CREDO_DEVICE_PATH /credo/serdes \
  --setenv PROFILE_SAI_INIT_CONFIG_FILE /credo/profile.xml \
  --setenv PROFILE_SAI_BOOT_TYPE "${BOOT_TYPE:-cold}" \
  "${fake[@]}" "${merge[@]}" "${stage[@]}" \
  /credo/fakephy "$@" > "$OUT/access.txt" 2> "$ERRF"
rc=$?
set -e
echo "exit $rc (124 = timeout)" | tee "$OUT/exit.txt"
echo "accesses: R $(grep -c '^R ' "$OUT/access.txt") W $(grep -c '^W ' "$OUT/access.txt")"
grep '^#' "$OUT/access.txt" | tail -5
