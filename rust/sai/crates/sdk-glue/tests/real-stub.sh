#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# sdk-glue's REAL path offline (tests/real_stub.rs): the feature "real" (Glue::Real -> real.rs ->
# the C ABI of sdk_glue.h) linked against a C stub of the header instead of sdk_glue.c + libsdk.
# Compiles tests/c/real_stub_threads.c (which includes tests/c/glue_stub.c) into
# libob_sdk_glue_stub.a, then runs
#   SDK_GLUE_LINK="<that archive> -lpthread" cargo test -p sdk-glue --features real --test real_stub
# In the package's build environment (nix/package.nix: cargo, bindgen's libclang, a C compiler).
#
# usage: crates/sdk-glue/tests/real-stub.sh [--valgrind] [--asan] [--target <triple>] [-- <test args>]
#   --valgrind   the test binary and the children it starts under valgrind's memcheck (any error,
#                definite or indirect leak: exit 99). valgrind from PATH or $VALGRIND.
#   --asan       the C side built with -fsanitize=address,undefined, its runtime preloaded into the
#                test process only (the Rust side is not instrumented: -Zsanitizer needs nightly).
#   --target T   cross build for T (big-endian ppc64: powerpc64-unknown-linux-gnuelfv2, the triple
#                nixpkgs' pkgsCross.ppc64 rustc has a standard library for) with $CC (the target's C
#                compiler, also the linker) and $AR; $RUSTC must have T's standard library, and
#                BINDGEN_EXTRA_CLANG_ARGS_<T, '-' as '_'> must point libclang at T's libc headers
#                (-nostdlibinc -resource-dir=<clang's> -idirafter <T's glibc-dev>/include). Run
#                through $RUNNER (default: qemu-ppc64 -L ${QEMU_LD_PREFIX:-/}), the test's own
#                children too (SDK_GLUE_STUB_RUNNER).
# The header: $SDK_GLUE_INC, $MDIO_SCRIPT_C_DIR, $SAI_OPENBCM_SRC, else the monorepo's copy (as
# sdk-glue-sys/build.rs looks). Build products: $REAL_STUB_OUT (default
# $CARGO_TARGET_DIR/real-stub/<variant>, CARGO_TARGET_DIR default target/).
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ws=$(cd "$here/../../.." && pwd)
valgrind=0 asan=0 target=""
while [ $# -gt 0 ]; do
    case "$1" in
        --valgrind) valgrind=1 ;;
        --asan) asan=1 ;;
        --target) target=$2; shift ;;
        --) shift; break ;;
        *) echo "real-stub.sh: unknown argument $1" >&2; exit 2 ;;
    esac
    shift
done

inc=""
for d in "${SDK_GLUE_INC:-}" "${MDIO_SCRIPT_C_DIR:-}" "${SAI_OPENBCM_SRC:-}" "$ws/../../ai_re/nix/packages/sai-openbcm/src"; do
    if [ -n "$d" ] && [ -f "$d/sdk_glue.h" ]; then inc=$d; break; fi
done
[ -n "$inc" ] || { echo "real-stub.sh: no sdk_glue.h (set SDK_GLUE_INC)" >&2; exit 2; }
export SDK_GLUE_INC=$inc

host=$(rustc -vV | sed -n 's/^host: //p')
triple=${target:-$host}
TRIPLE=$(echo "$triple" | tr 'a-z-' 'A-Z_')
variant=$triple
[ $asan = 1 ] && variant=$variant-asan
[ $valgrind = 1 ] && variant=$variant-valgrind
cc=${CC:-cc}
out=${REAL_STUB_OUT:-${CARGO_TARGET_DIR:-$ws/target}/real-stub/$variant}
mkdir -p "$out"

cflags=(-std=gnu11 -O1 -g -fPIC -Wall -Wextra -Werror -I"$inc")
link="$out/libob_sdk_glue_stub.a -lpthread"
runner=()
if [ $asan = 1 ]; then
    cflags+=(-fsanitize=address,undefined -fno-omit-frame-pointer -fno-sanitize-recover=all)
    link="$out/libob_sdk_glue_stub.a -lasan -lubsan -lpthread"
    # the runtime must come first in the process (the Rust binary is not instrumented)
    runner=(env "LD_PRELOAD=$("$cc" -print-file-name=libasan.so)"
        "ASAN_OPTIONS=detect_leaks=1:exitcode=98"
        "UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1:exitcode=97")
fi
if [ $valgrind = 1 ]; then
    vg=${VALGRIND:-$(command -v valgrind || true)}
    [ -n "$vg" ] || { echo "real-stub.sh: no valgrind (PATH or VALGRIND=)" >&2; exit 2; }
    runner+=("$vg" --quiet --trace-children=yes --error-exitcode=99 --leak-check=full
        --errors-for-leak-kinds=definite,indirect --show-leak-kinds=definite,indirect)
fi

"$cc" "${cflags[@]}" -c "$here/c/real_stub_threads.c" -o "$out/real_stub_threads.o"
ar=${AR:-ar}
rm -f "$out/libob_sdk_glue_stub.a"
"$ar" rcs "$out/libob_sdk_glue_stub.a" "$out/real_stub_threads.o"

cargo_args=(test -p sdk-glue --features real --test real_stub)
if [ -n "$target" ]; then
    cargo_args+=(--target "$target")
    export "CARGO_TARGET_${TRIPLE}_LINKER=$cc" "CC_$(echo "$target" | tr - _)=$cc" "SDK_GLUE_SYS_CC=$cc"
    if [ ${#runner[@]} -eq 0 ]; then
        read -r -a runner <<< "${RUNNER:-qemu-ppc64 -L ${QEMU_LD_PREFIX:-/}}"
    fi
    # the test re-executes itself: its children need the emulator too
    export SDK_GLUE_STUB_RUNNER="${runner[*]}"
fi
if [ ${#runner[@]} -gt 0 ]; then
    export "CARGO_TARGET_${TRIPLE}_RUNNER=${runner[*]}"
fi
echo "real-stub.sh: $variant, header $inc, SDK_GLUE_LINK=$link, runner: ${runner[*]:-none}" >&2
cd "$ws"
SDK_GLUE_LINK=$link exec cargo "${cargo_args[@]}" -- "$@"
