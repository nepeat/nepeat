# libortools.so.9 stand-in for the amd64 isai: libmkaecp.so (NEEDED by libsai)
# imports 35 OR-Tools 9.11 CP-SAT symbols (built by Marvell with
# _GLIBCXX_USE_CXX11_ABI=0 against abseil lts_20240722). We do not build
# OR-Tools 9.11: the functions here abort loudly if anything calls them (the
# CP-SAT solver is not on the SAI init path as far as the symbol use shows),
# the two TimeLimit constants carry upstream's values, and the abseil symbols
# come from the real abseil 20240722 (DT_NEEDED). The list is generated from
# the vendor library at build time, so a different isai cannot silently miss one.
{ lib, stdenv, abseil-cpp_202407, isaiUnpacked }:
stdenv.mkDerivation {
  pname = "libortools-stub";
  version = "9.11";
  dontUnpack = true;
  buildInputs = [ abseil-cpp_202407 ];
  buildPhase = ''
    runHook preBuild
    $NM -D --undefined-only ${isaiUnpacked}/isai/usr/lib/libmkaecp.so.1.0.0 \
      | awk '{print $2}' | sed 's/@.*//' \
      | grep -E '^(_ZN|_ZNK)19operations_research|^FLAGS_' | sort -u > syms
    {
      echo '#include <stdio.h>'
      echo '#include <stdlib.h>'
      echo 'static void die(const char *s) { fprintf(stderr, "libortools stub: %s called; OR-Tools is not provided (qemu-ipd/guest/ortools-stub.nix)\n", s); abort(); }'
      i=0
      while read -r s; do
        case "$s" in
          _ZN19operations_research9TimeLimit12kHistorySizeE)
            echo "const int d$i __asm__(\"$s\") = 100;" ;;
          _ZN19operations_research9TimeLimit20kSafetyBufferSecondsE)
            echo "const double d$i __asm__(\"$s\") = 1e-4;" ;;
          FLAGS_*)   # absl::Flag<bool> object: only read on the solver path
            echo "char d$i[256] __asm__(\"$s\") __attribute__((aligned(16))) = {0};" ;;
          *)
            echo "void f$i(void) __asm__(\"$s\"); void f$i(void) { die(\"$s\"); }" ;;
        esac
        i=$((i+1))
      done < syms
    } > stub.c
    test "$(wc -l < syms)" -gt 30
    $CC -shared -fPIC -O2 -o libortools.so.9 -Wl,-soname,libortools.so.9 stub.c \
      -Wl,--no-as-needed -labsl_raw_hash_set -labsl_hash -labsl_raw_logging_internal \
      -labsl_time -labsl_flags_internal
    runHook postBuild
  '';
  installPhase = ''
    install -Dm755 libortools.so.9 $out/lib/libortools.so.9
    ln -s libortools.so.9 $out/lib/libortools.so
    install -Dm644 syms $out/share/libortools-stub/symbols.txt
  '';
  meta.platforms = [ "x86_64-linux" ];
}
