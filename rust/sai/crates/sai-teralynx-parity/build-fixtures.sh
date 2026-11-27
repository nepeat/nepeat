#!/usr/bin/env bash
# The parity harness's fixtures (crate sai-teralynx-parity, README "Parity"): the mock libifcs, the taps, the
# oracle's script filter and the port maps, from this crate's c/, rust/sai/tests/teralynx-mock and
# ai_re/nix/packages/ifcs-oracle's tracer generator (ORACLE_SRC). The programs the harness runs on the
# libsai are given to it (the Rust test_ifcs, crates/sai-backend-teralynx/tests/ifcs_e2e; the Rust
# sai-script): the C++ ones this script used to build (test_ifcs, frontend_driver.cpp, sai_script.cpp, the
# C shim) were deleted with the C++ sai-teralynx in H5 on 2026-11-27.
#
#   build-fixtures.sh OUT SHIM IFCS_INC [NLOHMANN_INC]
#
#   OUT/mock/libifcs.so          the mock libifcs (rust/sai/tests/teralynx-mock; MOCK_LIBIFCS: its nix derivation)
#   OUT/bin/libsai-teralynx-shim.so   SHIM, the Rust shim (crates/ifcs-shim; put next to the libsai)
#   OUT/bin/libparity-tap.so     c/tap.c (LD_PRELOAD: every mock line into one stream)
#   OUT/bin/libparity-bytes.so   the byte tap: ifcs-oracle's gen_tracer.py wrappers over the generated header with
#                                c/bytes_rt.c (SAI_TERALYNX_IFCS_TRACE: every IFCS call's argument bytes into
#                                the same stream; PYTHON: the build platform's python3)
#   OUT/bin/oracle-prep          c/oracle_prep.cpp (only with NLOHMANN_INC)
#   OUT/cfg/                     port maps (8 ports for the driver scripts, 32 for the oracle's), init yaml
#
# CC / CXX: the target's compilers.
set -euo pipefail
out=$1 shim=$2 gen=$3 nlo=${4:-}
here=$(cd "$(dirname "$0")" && pwd)
cc=${CC:-cc} cxx=${CXX:-c++}
# the oracle's tracer generator and runtime header: ai_re/nix/packages/ifcs-oracle (ORACLE_SRC), moved
# unchanged from sai-teralynx/oracle (H5-1)
oracle=${ORACLE_SRC:-$here/../../../../ai_re/nix/packages/ifcs-oracle}
warn="-Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers"
mkdir -p "$out/bin" "$out/mock" "$out/cfg" "$out/obj"
cp -f "$shim" "$out/bin/libsai-teralynx-shim.so"
# the mock: MOCK_LIBIFCS (nix/teralynx-mock.nix, the one derivation of every user), else compiled from
# tests/teralynx-mock (MOCK_SRC) with the same flags
if [ -n "${MOCK_LIBIFCS:-}" ]; then
  cp -f "$MOCK_LIBIFCS" "$out/mock/libifcs.so"
else
  mock=${MOCK_SRC:-$here/../../tests/teralynx-mock}
  $cc -std=gnu11 -fPIC -shared $warn -Werror=incompatible-pointer-types -O2 -g -I"$gen" -I"$mock" \
    -Wl,--no-undefined -o "$out/mock/libifcs.so" "$mock"/*.c -lpthread
fi
$cc -std=gnu11 -fPIC -shared -Wall -Wextra -Werror -O2 -g -Wl,-soname,libparity-tap.so \
  -o "$out/bin/libparity-tap.so" "$here/c/tap.c" -ldl -lpthread
${PYTHON:-python3} -I -B "$oracle/gen_tracer.py" --header "$gen/ifcs_api.h" --abi parity-bytes -o "$out/obj/bytes_gen.c"
$cc -std=gnu11 -fPIC -shared -Wall -Werror -O2 -g -I"$oracle" -I"$gen" -Wl,-soname,libparity-bytes.so \
  -o "$out/bin/libparity-bytes.so" "$out/obj/bytes_gen.c" "$here/c/bytes_rt.c" -ldl -lpthread
if [ -n "$nlo" ]; then
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
