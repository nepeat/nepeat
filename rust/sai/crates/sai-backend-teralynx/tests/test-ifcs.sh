#!/usr/bin/env bash
# test_ifcs (ai_re/nix/packages/sai-teralynx/tests: test_ifcs.cpp + t_*.cpp + harness.h,
# UNMODIFIED, compiled as its Makefile does) against a Rust libsai with the teralynx feature, on
# the C mock libifcs (tests/mock/*.c, the Makefile's flags) with the C shim next to it (src/shim.c
# + src/ifcs_guard.c); then the IFCS call log of create_switch / remove_switch (tests/c/create_remove.c)
# of that libsai and, if given, of the C++ sai-teralynx libsai on the same mock: they must be equal.
#
#   tests/test-ifcs.sh OUT LIBSAI TERALYNX_SRC IFCS_INC SAI_INC [CPP_LIBSAI]
#
# PREBUILT=1: OUT already holds lib/ (libsai, shim, test_ifcs), mock/ and create_remove (a test
# bundle built for another target, run there: nix/teralynx-target-run.nix); nothing is compiled.
# CC / CXX: the compilers (default cc / c++). Every FAIL line of test_ifcs must be one of
# tests/test_ifcs-expected.txt (the areas not ported yet, G2; each line listed), and the run must
# end as that file's last line says; a FAIL of a ported area (main()'s refusals, switch init,
# ports, counters) is never listed, so it is red. An expected line that passes now is reported (not
# red): drop it from the list. Results: OUT/test_ifcs-result.txt.
set -u
out=$1 libsai=$2 tl=$3 gen=$4 sai=$5 cpp=${6:-}
here=$(cd "$(dirname "$0")" && pwd)
expected=${EXPECTED:-$here/test_ifcs-expected.txt}
cc=${CC:-cc} cxx=${CXX:-c++}
warn="-Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers"
mkdir -p "$out/lib" "$out/mock" "$out/cpp"
if [ "${PREBUILT:-}" != 1 ]; then
set -e
cp -f "$libsai" "$out/lib/libsai.so.1.0.0"
ln -sf libsai.so.1.0.0 "$out/lib/libsai.so.1"
ln -sf libsai.so.1 "$out/lib/libsai.so"
# the shim and the mock as sai-teralynx's Makefile builds them
$cc -std=gnu11 -fPIC -shared -Wall -fno-builtin -fno-tree-loop-distribute-patterns -O2 -g \
  -Wl,-soname,libsai-teralynx-shim.so -o "$out/lib/libsai-teralynx-shim.so" "$tl/src/shim.c" "$tl/src/ifcs_guard.c" -lpthread
$cc -std=gnu11 -fPIC -shared $warn -Werror=incompatible-pointer-types -O2 -g -I"$tl/src" -I"$gen" -I"$tl/tests/mock" \
  -Wl,--no-undefined -o "$out/mock/libifcs.so" "$tl"/tests/mock/*.c -lpthread
$cxx -std=c++17 $warn -O2 -g -I"$sai" -I"$sai/../experimental" -I"$tl/tests" -o "$out/lib/test_ifcs" \
  "$tl/tests/test_ifcs.cpp" "$tl"/tests/t_*.cpp "$out/lib/libsai.so.1.0.0" -Wl,-rpath,'$ORIGIN' -ldl -lpthread
$cc -std=gnu11 -Wall -Wextra -O1 -g -I"$sai" -I"$sai/../experimental" -o "$out/create_remove" "$here/c/create_remove.c" -ldl
set +e
fi

rm -rf "$out/ct"
"$out/lib/test_ifcs" "$out/mock/libifcs.so" "$out/ct" > "$out/test_ifcs.stdout" 2> "$out/test_ifcs.stderr"
rc=$?
# FAIL lines without the source directory
# FAIL lines without the source directory, and without the compared values of a CHECK_EQ (a check
# of an unported area may print uninitialised memory): file, line and expression decide
norm() { sed -E 's|^FAIL .*/tests/|FAIL |; s/ \([^()]* vs [^()]*\)$//'; }
grep '^FAIL ' "$out/test_ifcs.stderr" | norm > "$out/fails.txt"
if grep -q "^terminate called" "$out/test_ifcs.stderr"; then
  end="aborted: $(grep -A1 '^terminate called' "$out/test_ifcs.stderr" | tail -1 | sed 's/^ *//')"
else
  end="finished: $(tail -1 "$out/test_ifcs.stdout") (exit $rc)"
fi
grep '^FAIL ' "$expected" | norm > "$out/expected-fails.txt"
want_end=$(grep '^END ' "$expected" | sed 's/^END //')
red=0
{
  echo "test_ifcs on the Rust libsai: $(wc -l < "$out/fails.txt") FAIL lines, $end"
  echo "== FAIL lines per file (ported areas: test_ifcs.cpp, t_switch.cpp, t_port.cpp, t_counters.cpp)"
  sed -E 's/^FAIL ([a-z0-9_]+\.cpp):.*/\1/' "$out/fails.txt" | sort | uniq -c
  echo "== not in the expected list (a regression: red)"
  grep -vxF -f "$out/expected-fails.txt" "$out/fails.txt" || echo "(none)"
  echo "== expected, passing now (drop them from tests/test_ifcs-expected.txt)"
  grep -vxF -f "$out/fails.txt" "$out/expected-fails.txt" || echo "(none)"
} > "$out/test_ifcs-result.txt"
if grep -vxqF -f "$out/expected-fails.txt" "$out/fails.txt"; then red=1; fi
if [ "$end" != "$want_end" ]; then
  echo "== the run ended '$end', expected '$want_end' (red)" >> "$out/test_ifcs-result.txt"
  red=1
fi
# the ported areas' FAIL lines that are listed must be only the two of the diag shell's G2 commands
if grep -E '^FAIL (test_ifcs|t_switch|t_port)\.cpp' "$expected" >/dev/null; then
  echo "== the expected list names a ported area's check (red)" >> "$out/test_ifcs-result.txt"
  red=1
fi

# create_switch / remove_switch: the IFCS call log of the Rust libsai, and the C++'s
"$out/create_remove" "$out/lib/libsai.so.1.0.0" "$out/mock/libifcs.so" "$out/cr" > "$out/create_remove-rust.txt" 2> "$out/create_remove-rust.stderr"
echo "== create_remove (Rust): exit $?, $(grep -c . "$out/create_remove-rust.txt") lines" >> "$out/test_ifcs-result.txt"
if [ -n "$cpp" ]; then
  cp -f "$cpp" "$out/cpp/libsai.so.1.0.0"
  cp -f "$out/lib/libsai-teralynx-shim.so" "$out/cpp/"
  "$out/create_remove" "$out/cpp/libsai.so.1.0.0" "$out/mock/libifcs.so" "$out/cr" > "$out/create_remove-cpp.txt" 2> "$out/create_remove-cpp.stderr"
  echo "== create_remove (C++): exit $?" >> "$out/test_ifcs-result.txt"
  if diff -u "$out/create_remove-cpp.txt" "$out/create_remove-rust.txt" > "$out/create_remove.diff"; then
    echo "== create_switch / remove_switch IFCS calls: identical ($(grep -c '^ifcs_' "$out/create_remove-rust.txt") calls)" >> "$out/test_ifcs-result.txt"
  else
    echo "== create_switch / remove_switch IFCS calls DIFFER (red):" >> "$out/test_ifcs-result.txt"
    cat "$out/create_remove.diff" >> "$out/test_ifcs-result.txt"
    red=1
  fi
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
