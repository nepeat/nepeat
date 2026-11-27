#!/usr/bin/env bash
# The Rust libsai with the teralynx feature on the C mock libifcs (rust/sai/tests/teralynx-mock,
# nix/teralynx-mock.nix) with the Rust shim next to it (crates/ifcs-shim): the IFCS call log of
# create_switch / remove_switch (tests/c/create_remove.c) and the board's post-init register list on the mock's
# registers. The call log is compared with the C++ libsai's (tests/c/create_remove-cpp.txt, its golden: the C++
# sai-teralynx was deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0). The C++ test_ifcs that this
# script used to compile and run is ported to Rust: tests/ifcs_e2e, compared with
# tests/ifcs_e2e/golden/test_ifcs-checks-cpp.txt.
#
#   tests/test-ifcs.sh OUT LIBSAI SHIM IFCS_INC SAI_INC
#
# LIBSAI: the Rust libsai.so; SHIM: libsai-teralynx-shim.so (the Rust one); IFCS_INC: the generated ifcs_api.h's
# directory (only to compile the mock when MOCK_LIBIFCS is not given); SAI_INC: the SAI headers.
# PREBUILT=1: OUT already holds lib/ (libsai, shim), mock/ and create_remove (a test bundle built for another
# target, run there: nix/teralynx-target-run.nix); nothing is compiled. CC: the compiler (default cc).
# Results: OUT/test_ifcs-result.txt; exit 1 if one is red.
set -u
out=$1 libsai=$2 shim=$3 gen=$4 sai=$5
here=$(cd "$(dirname "$0")" && pwd)
cc=${CC:-cc}
warn="-Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers"
mkdir -p "$out/lib" "$out/mock"
if [ "${PREBUILT:-}" != 1 ]; then
set -e
cp -f "$libsai" "$out/lib/libsai.so.1.0.0"
ln -sf libsai.so.1.0.0 "$out/lib/libsai.so.1"
ln -sf libsai.so.1 "$out/lib/libsai.so"
cp -f "$shim" "$out/lib/libsai-teralynx-shim.so"
# the mock: MOCK_LIBIFCS (nix/teralynx-mock.nix, the one derivation of every user), else compiled from
# tests/teralynx-mock (MOCK_SRC) with the same flags
if [ -n "${MOCK_LIBIFCS:-}" ]; then
  cp -f "$MOCK_LIBIFCS" "$out/mock/libifcs.so"
else
  mock=${MOCK_SRC:-$here/../../../tests/teralynx-mock}
  $cc -std=gnu11 -fPIC -shared $warn -Werror=incompatible-pointer-types -O2 -g -I"$gen" -I"$mock" \
    -Wl,--no-undefined -o "$out/mock/libifcs.so" "$mock"/*.c -lpthread
fi
$cc -std=gnu11 -Wall -Wextra -O1 -g -I"$sai" -I"$sai/../experimental" -o "$out/create_remove" "$here/c/create_remove.c" -ldl
set +e
fi

red=0
: > "$out/test_ifcs-result.txt"
# create_switch / remove_switch: the IFCS call log of the Rust libsai
"$out/create_remove" "$out/lib/libsai.so.1.0.0" "$out/mock/libifcs.so" "$out/cr" > "$out/create_remove-rust.txt" 2> "$out/create_remove-rust.stderr"
rc=$?
echo "== create_remove (Rust): exit $rc, $(grep -c . "$out/create_remove-rust.txt") lines" >> "$out/test_ifcs-result.txt"
[ $rc -eq 0 ] || red=1
# ... equal to the C++ library's (the golden; GOLDEN_CR: another copy, the bundle's)
golden=${GOLDEN_CR:-$here/c/create_remove-cpp.txt}
if sed "s|$out/cr|<cr>|g" "$out/create_remove-rust.txt" | diff -u <(grep -v '^#' "$golden") - > "$out/create_remove.diff"; then
  echo "== create_switch / remove_switch IFCS calls: identical to the C++ golden ($(grep -c '^ifcs_' "$out/create_remove-rust.txt") calls)" >> "$out/test_ifcs-result.txt"
else
  echo "== create_switch / remove_switch IFCS calls DIFFER from the C++ golden (red):" >> "$out/test_ifcs-result.txt"
  cat "$out/create_remove.diff" >> "$out/test_ifcs-result.txt"
  red=1
fi
# the board's post-init register list on the mock's registers (Rust only: no C++ counterpart)
regs=${REGS:-$here/post-init-regs.txt}
watch=${WATCH:-$here/diag-regs.txt}
"$out/create_remove" "$out/lib/libsai.so.1.0.0" "$out/mock/libifcs.so" "$out/cr-regs" "$regs" "$watch" > "$out/regs.txt" 2> "$out/regs.stderr"
want="create_switch: 0
regs 1: 0x13514=0x808 0x13530=0x20 0x13538=0x18eec2 0x1353c=0x2
diag: post-init registers: 4 listed, 4 applied, 0 mismatched, 0 failed
diag: 0x13514 mask 0x0000080f want 0x00000808 now 0x00000808 ok
diag: 0x13538 mask 0xffffffff want 0x0018eec2 now 0x0018eec2 ok
diag: 0x13530 mask 0xffffffff want 0x00000020 now 0x00000020 ok
diag: 0x1353c mask 0x00000002 want 0x00000002 now 0x00000002 ok
diag: watch registers: 5 (SAI_TERALYNX_DIAG_REGS)
diag: 0x13514 = 0x00000808
diag: 0x13530 = 0x00000020
diag: 0x13534 = 0x00000000
diag: 0x13538 = 0x0018eec2
diag: 0x13600: read failed (11)
remove_switch 1: 0
create_switch 2: 0
regs 2: 0x13514=0xffff0808 0x13530=0x20 0x13538=0x18eec2 0x1353c=0x7
remove_switch 2: 0
create_switch 3: 0
regs 3: 0x13514=0xffff0808 0x13530=0x20 0x13538=0x18eec2 0x1353c=0x7
remove_switch 3: 0
sai_api_uninitialize: 0"
if [ "$(cat "$out/regs.txt")" = "$want" ] && grep -q '0x13530 reads 0xffffffff after writing 0x20 (mask 0xffffffff): mismatch' "$out/regs.stderr"; then
  echo "== post-init registers: written, read back, re-applied on re-create, a read-back mismatch logged; the diag regs answer (list + watch list, live) as expected" >> "$out/test_ifcs-result.txt"
else
  { echo "== post-init registers: NOT as expected (red)"; cat "$out/regs.txt"; grep -i 'regist\|mismatch' "$out/regs.stderr"; } >> "$out/test_ifcs-result.txt"
  red=1
fi
cat "$out/test_ifcs-result.txt"
exit $red
