# libsai-teralynx-shim.so in Rust (crates ifcs-shim over ifcs-guard; README "Teralynx shim"), checked
# against the C one it replaces (ai_re/nix/packages/sai-teralynx/src/{shim.c,ifcs_guard.c}):
#   native:  pkgs.callPackage ./ifcs-shim.nix { saiRs = <./package.nix>; saiTeralynx = <sai-teralynx package>; }
#   aarch64: pkgs.pkgsCross.aarch64-multiplatform.callPackage ./ifcs-shim.nix {
#              saiRs = <./package.nix for those pkgs>; saiTeralynx = <sai-teralynx for those pkgs>;
#              runnerPkgs = <nixpkgs for system aarch64-linux>;   # runs the aarch64 binaries (binfmt / extra-platforms)
#              scriptsFrom = <the native build of this file>; }   # its differential scripts, replayed there
# saiTeralynx is only read: its own derivation is not changed (overrideAttrs makes new ones).
#
# $out:
#   lib/libsai-teralynx-shim.so      the Rust library (soname libsai-teralynx-shim.so)
#   share/sai-rs/libsai-teralynx-shim.txt   build report: soname, NEEDED, TLS, exports, the
#                                    disassembly of tl_devmem_memcpy, undefined symbols by version
#   share/sai-rs/ifcs-shim-scripts/  (native) every differential script (corpus + generated)
#   share/sai-rs/test_ifcs-*.txt     the drop-in runs below
#
# Checks (a failure fails the build):
#   - always (no execution): soname; the exported (defined dynamic) symbols equal the C library's
#     (saiTeralynx's lib/libsai-teralynx-shim.so, built by its Makefile for the same platform);
#     tl_devmem_memcpy has no SIMD / FP register, no pair or multi-register load / store, no call
#     and no branch out of itself (no memcpy), only single loads / stores of the selected widths
#   - native: the crates' fmt, clippy -D warnings and tests, the differential against the C library
#     compiled from saiTeralynx's sources (required: it cannot skip here), on the release library;
#     drop-in: sai-teralynx's own test_ifcs (built by its Makefile, unmodified) with this library in
#     place of build/libsai-teralynx-shim.so: 1047 checks, 0 failed (t_learn drives the guard through
#     the mock libifcs' PLT and the C++ loader's GOT redirect)
#   - aarch64 with runnerPkgs: under emulation, test_ifcs cross-built (R_AARCH64 GOT relocations) with
#     the C shim (baseline) and with this library: 1047 / 0 each; the differential harness replays
#     scriptsFrom's scripts against both aarch64 libraries: every trace equal
{
  lib,
  stdenv,
  runCommand,
  saiRs,
  saiTeralynx,
  runnerPkgs ? null,
  scriptsFrom ? null,
}:
let
  canRun = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  bin = "${stdenv.cc.bintools.bintools}/bin/${stdenv.cc.targetPrefix}";
  cc = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
  triple = stdenv.hostPlatform.rust.rustcTarget;
  arch = stdenv.hostPlatform.parsed.cpu.name;
  cShimSrc = "${saiTeralynx.src}/src";
  cShim = "${saiTeralynx}/lib/libsai-teralynx-shim.so";
  harnessSrc = builtins.path { path = ../crates/ifcs-shim/tests/c/guard_harness.c; name = "guard_harness.c"; };

  # tl_devmem_memcpy's instructions that must not appear (per architecture), and its loads / stores
  forbidden = {
    aarch64 = "(^|[[:space:]])(ldp|stp|ldnp|stnp|ld[1-4]|st[1-4]|ldxp|stxp|ldrh|strh|ldursh|bl|blr|br|ldr[[:space:]]+[qdsh][0-9]|str[[:space:]]+[qdsh][0-9])([[:space:]]|$)|v[0-9]+\\.|(^|[^_a-z])memcpy|@plt";
    x86_64 = "(^|[[:space:]])(call|rep|movs[bwdq]?|movdq[au]|movups|movaps)([[:space:]]|$)|%?[xyz]mm[0-9]|(^|[^_a-z])memcpy|@plt";
  }.${arch};

  shim = saiRs.overrideAttrs (old: {
    pname = "sai-teralynx-shim-rs";
    cargoBuildFlags = [ "-p" "ifcs-shim" ];
    cargoTestFlags = [ "-p" "ifcs-guard" "-p" "ifcs-shim" ];
    # only what these two crates need: not the board tables or the frozen frontend the workspace
    # build's goldens read (they would make this build depend on the AS77xx packages)
    env = builtins.removeAttrs old.env [ "PHY_SEQ_TABLES" "SAI_OPENBCM_SRC" ] // {
      IFCS_SHIM_C_SRC = cShimSrc;
      IFCS_SHIM_DIFF_REQUIRE = "1";
    };
    preCheck = ''
      cargo fmt -p ifcs-guard -p ifcs-shim --check
      cargo clippy --offline -p ifcs-guard -p ifcs-shim --all-targets --profile "$cargoCheckType" -- -D warnings
      # the differential runs against the library that ships (this build phase's)
      export IFCS_SHIM_LIB=$PWD/target/${triple}/$cargoBuildType/libsai_teralynx_shim.so
      test -f "$IFCS_SHIM_LIB"
      export IFCS_SHIM_DIFF_DUMP=$NIX_BUILD_TOP/ifcs-shim-scripts
    '';
    postCheck = "";
    postInstall = ''
      so=$out/lib/libsai-teralynx-shim.so
      mv $out/lib/libsai_teralynx_shim.so $so
      report=$out/share/sai-rs/libsai-teralynx-shim.txt
      mkdir -p $out/share/sai-rs
      exports() { ${bin}nm -D --defined-only "$1" | awk '{ print $3 }' | LC_ALL=C sort; }
      exports $so > exports.rust
      exports ${cShim} > exports.c
      # tl_devmem_memcpy: its address and size from the dynamic symbol table, then its instructions
      read -r start size < <(${bin}nm -D -S --defined-only $so | awk '$4 == "tl_devmem_memcpy" { print $1, $2 }')
      stop=$(printf '%x' $((0x$start + 0x$size)))
      ${bin}objdump -d --no-show-raw-insn --start-address=0x$start --stop-address=0x$stop $so \
        | sed -n '/<tl_devmem_memcpy>:/,$p' > memcpy.dis
      read -r cstart csize < <(${bin}nm -D -S --defined-only ${cShim} | awk '$4 == "tl_devmem_memcpy" { print $1, $2 }')
      ${bin}objdump -d --no-show-raw-insn --start-address=0x$cstart --stop-address=$(printf '0x%x' $((0x$cstart + 0x$csize))) ${cShim} \
        | sed -n '/<tl_devmem_memcpy>:/,$p' > memcpy-c.dis
      {
        echo "libsai-teralynx-shim.so (Rust: crates ifcs-shim + ifcs-guard), ${stdenv.hostPlatform.config}"
        echo "size: $(stat -c %s $so) bytes (the C one: $(stat -c %s ${cShim}))"
        echo "== dynamic section"
        ${bin}readelf -d $so | grep -E 'SONAME|NEEDED|RUNPATH|FLAGS|TEXTREL'
        echo "== TLS segment (dynamic TLS of std; the guard's paths do not touch it: differential allocation counts)"
        ${bin}readelf -lW $so | grep -E 'TLS' || echo "none"
        echo "== exported symbols ($(wc -l < exports.rust); the C library: $(wc -l < exports.c))"
        cat exports.rust
        echo "== undefined symbols: $(${bin}nm -D --undefined-only $so | wc -l), by version"
        ${bin}nm -D --undefined-only $so | sed -n 's/.*@\(.*\)/\1/p' | LC_ALL=C sort | uniq -c
        echo "== tl_devmem_memcpy (Rust), $((0x$size)) bytes"
        cat memcpy.dis
        echo "== tl_devmem_memcpy (C, -fno-builtin -fno-tree-loop-distribute-patterns), $((0x$csize)) bytes"
        cat memcpy-c.dis
      } > $report
      cat $report

      ${bin}readelf -d $so | grep -q 'SONAME.*\[libsai-teralynx-shim.so\]' || { echo "wrong soname"; exit 1; }
      diff -u exports.c exports.rust || { echo "exported symbols differ from the C library's"; exit 1; }
      grep -q 'ret' memcpy.dis || { echo "tl_devmem_memcpy: no disassembly"; exit 1; }
      # the frame record (rustc keeps frame pointers here: x29 / x30 pushed and popped on the stack,
      # aarch64's one pair access) is not a copy access; any other pair, SIMD or call fails
      if grep -vE '(stp|ldp)[[:space:]]+x29, x30, \[sp' memcpy.dis | grep -nE '${forbidden}'; then
        echo "tl_devmem_memcpy: forbidden instructions (above)"; exit 1
      fi
      # every branch stays inside the function (no tail call into memcpy or anything else)
      awk -v lo=$((0x$start)) -v hi=$((0x$stop)) '
        /^ *[0-9a-f]+:/ {
          for (i = 2; i <= NF; i++) if ($i ~ /^</) { t = strtonum("0x" $(i - 1)); if (t < lo || t >= hi) { print "branch out: " $0; bad = 1 } }
        }
        END { exit bad }' memcpy.dis || { echo "tl_devmem_memcpy branches out of itself"; exit 1; }
      ''${IFCS_SHIM_DIFF_DUMP:+cp -r $IFCS_SHIM_DIFF_DUMP $out/share/sai-rs/ifcs-shim-scripts}
    '';
    passthru = (old.passthru or { }) // { inherit cShim; };
    meta = old.meta // {
      description = "libsai-teralynx-shim.so in Rust: CPython stubs, Device-memory copy, the learn-ring read guard";
    };
  });

  # sai-teralynx's test_ifcs, exactly as its Makefile builds it (without the frontend's own tests)
  testBuild = saiTeralynx.overrideAttrs (old: {
    pname = "sai-teralynx-test-ifcs";
    makeFlags = lib.filter (f: !lib.hasPrefix "FRONTEND_TESTS=" f) old.makeFlags;
    buildFlags = [ "build/libsai.so.1.0.0" "build/test_ifcs" "build/mock/libifcs.so" "build/libsai-teralynx-shim.so" ];
  });

  # native: test_ifcs with the Rust library next to libsai (where the C++ loader dlopens it)
  dropIn = testBuild.overrideAttrs (old: {
    pname = "sai-teralynx-shim-rs-drop-in";
    postBuild = (old.postBuild or "") + ''
      install -m755 ${shim}/lib/libsai-teralynx-shim.so build/libsai-teralynx-shim.so
    '';
    doCheck = true;
    checkPhase = ''
      runHook preCheck
      cmp build/libsai-teralynx-shim.so ${shim}/lib/libsai-teralynx-shim.so
      ./build/test_ifcs $PWD/build/mock/libifcs.so $PWD/build/check-tmp > test_ifcs.txt 2>&1 || true
      cat test_ifcs.txt
      grep -qx 'test_ifcs: 1047 checks, 0 failed' test_ifcs.txt || { echo "test_ifcs with the Rust shim failed"; exit 1; }
      runHook postCheck
    '';
    installPhase = ''
      mkdir -p $out/share/sai-rs
      { echo "test_ifcs (sai-teralynx, unmodified) with the Rust libsai-teralynx-shim.so ${shim}/lib/libsai-teralynx-shim.so"
        cat test_ifcs.txt; } > $out/share/sai-rs/test_ifcs-rust-shim.txt
    '';
  });

  # aarch64: the cross-built test_ifcs, mock, C++ libsai, C shim and the differential harness
  crossTests = testBuild.overrideAttrs (old: {
    pname = "sai-teralynx-test-ifcs-${arch}";
    postBuild = (old.postBuild or "") + ''
      ${cc} -std=gnu11 -O1 -g -Wall -Wextra -Werror -rdynamic -o build/guard_harness ${harnessSrc} -ldl -lpthread
    '';
    doCheck = false;
    installPhase = ''
      mkdir -p $out/test/mock
      cp build/libsai.so.1.0.0 build/test_ifcs build/guard_harness $out/test/
      ln -s libsai.so.1.0.0 $out/test/libsai.so.1
      cp build/mock/libifcs.so $out/test/mock/
      cp build/libsai-teralynx-shim.so $out/test/libsai-teralynx-shim-c.so
    '';
    dontFixup = true;
  });

  emulated = runnerPkgs.runCommand "sai-teralynx-shim-rs-${arch}-emulated" { } ''
    set -euo pipefail
    mkdir -p $out/share/sai-rs
    for which in c rust; do
      rm -rf t
      cp -r ${crossTests}/test t
      chmod -R u+w t
      if [ $which = rust ]; then cp ${shim}/lib/libsai-teralynx-shim.so t/libsai-teralynx-shim.so
      else cp t/libsai-teralynx-shim-c.so t/libsai-teralynx-shim.so; fi
      (cd t && ./test_ifcs $PWD/mock/libifcs.so $PWD/tmp) > test_ifcs-$which.txt 2>&1 || true
      tail -n 3 test_ifcs-$which.txt
      grep -qx 'test_ifcs: 1047 checks, 0 failed' test_ifcs-$which.txt || { echo "test_ifcs ($which shim, ${arch}) failed"; cat test_ifcs-$which.txt; exit 1; }
      cp test_ifcs-$which.txt $out/share/sai-rs/test_ifcs-${arch}-$which-shim.txt
    done
    ${lib.optionalString (scriptsFrom != null) ''
      n=0; lines=0; allowed=0
      for s in ${scriptsFrom}/share/sai-rs/ifcs-shim-scripts/*.txt; do
        for which in c rust; do
          lib=${crossTests}/test/libsai-teralynx-shim-c.so
          [ $which = rust ] && lib=${shim}/lib/libsai-teralynx-shim.so
          rm -rf w-$which; mkdir w-$which
          ${crossTests}/test/guard_harness $lib $s w-$which > trace-$which.txt
        done
        # the one deliberate difference (README "Teralynx shim", tests/differential.rs
        # compare_allowing_text_cap): a counters' text of 4096 bytes or more (unescaped) is written
        # (a "  prom" line) and answered ("  text") by the Rust guard only, the C's text is then ""
        awk '
          function ulen(s,  t) { t = s; gsub(/\\x[0-9a-f][0-9a-f]/, "X", t); gsub(/\\./, "X", t); return length(t) }
          /^  prom / && ulen(substr($0, 8)) >= 4096 { a++; next }
          /^  text / && ulen(substr($0, 8)) >= 4096 { print "  text "; a++; next }
          { print }
          END { print a + 0 > "allowed.txt" }' trace-rust.txt > trace-rust-c-cap.txt
        allowed=$((allowed + $(cat allowed.txt)))
        if ! cmp -s trace-c.txt trace-rust-c-cap.txt; then
          echo "differential (${arch}): $(basename $s) differs"; diff trace-c.txt trace-rust.txt | head -40; exit 1
        fi
        if grep -qE 'NOT in do_malloc|NOT ZEROED|WRONG|OTHER' trace-c.txt; then echo "harness check failed in $s"; exit 1; fi
        n=$((n + 1)); lines=$((lines + $(wc -l < trace-c.txt)))
      done
      echo "ifcs-shim differential (${arch}, emulated): $n scripts, $lines trace lines equal; $allowed lines differ by the one deliberate difference (counters' text of 4096 bytes or more: Rust only)" | tee $out/share/sai-rs/differential-${arch}.txt
    ''}
  '';
in
runCommand "sai-teralynx-shim-rs-${arch}" {
  passthru = { inherit shim; } // lib.optionalAttrs canRun { inherit dropIn; } // lib.optionalAttrs (runnerPkgs != null) { inherit emulated crossTests; };
} ''
  mkdir -p $out/lib $out/share/sai-rs
  cp ${shim}/lib/libsai-teralynx-shim.so $out/lib/
  cp -r ${shim}/share/sai-rs/. $out/share/sai-rs/
  ${lib.optionalString canRun "cp ${dropIn}/share/sai-rs/* $out/share/sai-rs/"}
  ${lib.optionalString (runnerPkgs != null) "cp -r ${emulated}/share/sai-rs/. $out/share/sai-rs/"}
''
