#!/usr/bin/env bash
# The parity harness's programs (crate sai-teralynx-parity, README "Parity"), built from sources that
# stay UNMODIFIED (ai_re/nix/packages/sai-teralynx: tests/, tests/mock/, src/shim.c + src/ifcs_guard.c,
# frontend/tests/frontend_driver.cpp, oracle/sai_script.cpp) and this crate's c/:
#
#   build-fixtures.sh OUT TERALYNX_SRC IFCS_INC SAI_INC LINK_LIBSAI [NLOHMANN_INC]
#
#   OUT/mock/libifcs.so          the mock libifcs, as sai-teralynx's Makefile builds it
#   OUT/bin/libsai-teralynx-shim.so   the C shim, as the Makefile builds it (put next to each libsai)
#   OUT/bin/libparity-tap.so     c/tap.c (LD_PRELOAD: every mock line into one stream)
#   OUT/bin/test_ifcs-sections   test_ifcs.cpp + t_*.cpp (the Makefile's flags) + c/sections.cpp,
#                                -Wl,--wrap for each area main() calls (c/wrap-symbols.txt)
#   OUT/bin/tl-frontend-driver   frontend_driver.cpp + c/tl_port.cpp, a SAI C API client of any libsai.so.1
#   OUT/bin/sai-script, oracle-prep   the oracle's runner and c/oracle_prep.cpp (only with NLOHMANN_INC)
#   OUT/cfg/                     port maps (8 ports for the driver scripts, 32 for the oracle's), init yaml
#
# The programs link against LINK_LIBSAI (any libsai.so.1 with the SAI exports: the soname is what
# counts) and find the library under test through LD_LIBRARY_PATH at run time. CC / CXX: compilers.
set -euo pipefail
out=$1 tl=$2 gen=$3 sai=$4 link=$5 nlo=${6:-}
here=$(cd "$(dirname "$0")" && pwd)
cc=${CC:-cc} cxx=${CXX:-c++}
warn="-Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers"
mkdir -p "$out/bin" "$out/mock" "$out/cfg" "$out/obj" "$out/link"
cp -f "$link" "$out/link/libsai.so.1"
$cc -std=gnu11 -fPIC -shared -Wall -fno-builtin -fno-tree-loop-distribute-patterns -O2 -g \
  -Wl,-soname,libsai-teralynx-shim.so -o "$out/bin/libsai-teralynx-shim.so" "$tl/src/shim.c" "$tl/src/ifcs_guard.c" -lpthread
$cc -std=gnu11 -fPIC -shared $warn -Werror=incompatible-pointer-types -O2 -g -I"$tl/src" -I"$gen" -I"$tl/tests/mock" \
  -Wl,--no-undefined -o "$out/mock/libifcs.so" "$tl"/tests/mock/*.c -lpthread
$cc -std=gnu11 -fPIC -shared -Wall -Wextra -Werror -O2 -g -Wl,-soname,libparity-tap.so \
  -o "$out/bin/libparity-tap.so" "$here/c/tap.c" -ldl -lpthread
wraps=$(grep -v '^#' "$here/c/wrap-symbols.txt" | sed 's/^/-Wl,--wrap=/' | tr '\n' ' ')
# shellcheck disable=SC2086
$cxx -std=c++17 $warn -O2 -g -I"$sai" -I"$sai/../experimental" -I"$tl/tests" -o "$out/bin/test_ifcs-sections" \
  "$tl/tests/test_ifcs.cpp" "$tl"/tests/t_*.cpp "$here/c/sections.cpp" $wraps -L"$out/link" -l:libsai.so.1 -ldl -lpthread
# every wrapped area must exist (a renamed area would silently lose its section)
for s in $(grep -v '^#' "$here/c/wrap-symbols.txt"); do
  nm "$out/bin/test_ifcs-sections" | grep -q " T $s\$" || { echo "build-fixtures: $s is not an area of test_ifcs" >&2; exit 1; }
  nm "$out/bin/test_ifcs-sections" | grep -q " T __wrap_$s\$" || { echo "build-fixtures: $s is not wrapped" >&2; exit 1; }
done
fe=$tl/frontend
awk -f "$fe/tests/driver_names.awk" $(ls "$sai"/sai*.h | LC_ALL=C sort) > "$out/obj/driver_names.inc"
$cxx -std=c++17 -O1 -w -ffunction-sections -fdata-sections -I"$sai" -I"$sai/../experimental" -I"$fe/src" -I"$fe/tests" \
  -I"$out/obj" -c "$fe/tests/frontend_driver.cpp" -o "$out/obj/frontend_driver.o"
$cxx -std=c++17 -O1 -Wall -Wextra -Werror -Wno-unused-parameter -ffunction-sections -fdata-sections -I"$sai" -I"$sai/../experimental" \
  -I"$fe/src" -I"$fe/tests" -I"$gen" -I"$tl/src" -c "$here/c/tl_port.cpp" -o "$out/obj/tl_port.o"
# --gc-sections: the driver's factory over the frontend's fake backend is never called (the library
# has its own backend) and goes with the recorder it names
$cxx -o "$out/bin/tl-frontend-driver" "$out/obj/frontend_driver.o" "$out/obj/tl_port.o" -Wl,--gc-sections \
  -Wl,--wrap=sai_api_query -L"$out/link" -l:libsai.so.1 -ldl -lpthread
if [ -n "$nlo" ]; then
  $cxx -std=c++17 -O2 -Wall -I"$sai" -I"$sai/../experimental" -I"$nlo" -o "$out/bin/sai-script" "$tl/oracle/sai_script.cpp" \
    -L"$out/link" -l:libsai.so.1 -ldl -lpthread
  $cxx -std=c++17 -O2 -Wall -Wextra -Werror -I"$nlo" -o "$out/bin/oracle-prep" "$here/c/oracle_prep.cpp"
fi
{
  echo "# devport lanes speed fec: 8 ports of 4 lanes (the driver scripts' fake default: 8 ports)"
  for i in $(seq 1 8); do echo "$i $(seq -s, $(( (i - 1) * 4 + 1 )) $(( i * 4 ))) 100000 rs"; done
} > "$out/cfg/ports8.map"
{
  echo "# devport lanes speed fec: 32 ports of 8 lanes at 400G (the CS8210's front panel)"
  for i in $(seq 1 32); do echo "$i $(seq -s, $(( (i - 1) * 8 + 1 )) $(( i * 8 ))) 400000 rs"; done
} > "$out/cfg/ports32.map"
printf '# generated init config\nifcs:\n  options:\n    log_level: "info"\n' > "$out/cfg/init.yaml"
echo "build-fixtures: $out"
