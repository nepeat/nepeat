# libsai-teralynx-shim.so in Rust (crates ifcs-shim over ifcs-guard; README "Teralynx shim"), held to the golden of
# the C one it replaced (shim.c + ifcs_guard.c of the C++ sai-teralynx, deleted in H5 on 2026-11-27, last in the tree
# at commit 767d9b0):
#   native:  pkgs.callPackage ./ifcs-shim.nix { saiRs = <./package.nix>; }
#   aarch64: pkgs.pkgsCross.aarch64-multiplatform.callPackage ./ifcs-shim.nix {
#              saiRs = <./package.nix for those pkgs>;
#              runnerPkgs = <nixpkgs for system aarch64-linux>;   # runs the aarch64 binaries (binfmt / extra-platforms)
#              scriptsFrom = <the native build of this file>; }   # its differential scripts and traces, replayed there
#
# $out:
#   lib/libsai-teralynx-shim.so      the Rust library (soname libsai-teralynx-shim.so)
#   share/sai-rs/libsai-teralynx-shim.txt   build report: soname, NEEDED, TLS, exports, the
#                                    disassembly of tl_devmem_memcpy, undefined symbols by version
#   share/sai-rs/ifcs-shim-scripts/  (native) every differential script (corpus + generated)
#   share/sai-rs/ifcs-shim-traces/   (native) the harness's trace of each with this library
#
# Checks (a failure fails the build):
#   - always (no execution): soname; the exported (defined dynamic) symbols equal the C library's, as
#     recorded in ./golden/sai-teralynx-package.txt; tl_devmem_memcpy has no SIMD / FP register, no pair or
#     multi-register load / store, no call and no branch out of itself (no memcpy), only single loads / stores
#     of the selected widths
#   - native: the crates' fmt, clippy -D warnings and tests, the differential against the golden of the C
#     library (crates/ifcs-shim/tests/differential.rs, tests/golden/guard-traces-cpp.txt; required here: the
#     library is given), on the release library; then the harness's trace of every script with it
#   - aarch64 with runnerPkgs: under emulation, the harness replays scriptsFrom's scripts against the aarch64
#     library: every trace equal to the native one (which the native build holds to the golden)
# The drop-in runs (the learn-ring guard driven through the mock libifcs' PLT and the loader's GOT redirect:
# the Rust test_ifcs's learn_guard section) are ./libsai-teralynx.nix's, with this library next to the libsai.
{
  lib,
  stdenv,
  runCommand,
  saiRs,
  runnerPkgs ? null,
  scriptsFrom ? null,
}:
let
  canRun = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  bin = "${stdenv.cc.bintools.bintools}/bin/${stdenv.cc.targetPrefix}";
  cc = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
  triple = stdenv.hostPlatform.rust.rustcTarget;
  arch = stdenv.hostPlatform.parsed.cpu.name;
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
    # only what these two crates need: not the board tables the workspace build's goldens read (they
    # would make this build depend on the AS77xx packages)
    env = builtins.removeAttrs old.env [ "PHY_SEQ_TABLES" ];
    preCheck = ''
      cargo fmt -p ifcs-guard -p ifcs-shim --check
      cargo clippy --offline -p ifcs-guard -p ifcs-shim --all-targets --profile "$cargoCheckType" -- -D warnings
      # the golden differential runs against the library that ships (this build phase's)
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
      # the C library's exports, as recorded when it was deleted
      awk '/^== / { on = ($0 == "== libsai-teralynx-shim.so exports") ; next } on && NF' ${./golden/sai-teralynx-package.txt} > exports.c
      test -s exports.c
      # tl_devmem_memcpy: its address and size from the dynamic symbol table, then its instructions
      read -r start size < <(${bin}nm -D -S --defined-only $so | awk '$4 == "tl_devmem_memcpy" { print $1, $2 }')
      stop=$(printf '%x' $((0x$start + 0x$size)))
      ${bin}objdump -d --no-show-raw-insn --start-address=0x$start --stop-address=0x$stop $so \
        | sed -n '/<tl_devmem_memcpy>:/,$p' > memcpy.dis
      {
        echo "libsai-teralynx-shim.so (Rust: crates ifcs-shim + ifcs-guard), ${stdenv.hostPlatform.config}"
        echo "size: $(stat -c %s $so) bytes"
        echo "== dynamic section"
        ${bin}readelf -d $so | grep -E 'SONAME|NEEDED|RUNPATH|FLAGS|TEXTREL'
        echo "== TLS segment (dynamic TLS of std; the guard's paths do not touch it: differential allocation counts)"
        ${bin}readelf -lW $so | grep -E 'TLS' || echo "none"
        echo "== exported symbols ($(wc -l < exports.rust); the C library's, recorded: $(wc -l < exports.c))"
        cat exports.rust
        echo "== undefined symbols: $(${bin}nm -D --undefined-only $so | wc -l), by version"
        ${bin}nm -D --undefined-only $so | sed -n 's/.*@\(.*\)/\1/p' | LC_ALL=C sort | uniq -c
        echo "== tl_devmem_memcpy (Rust), $((0x$size)) bytes"
        cat memcpy.dis
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
      if [ -n "''${IFCS_SHIM_DIFF_DUMP:-}" ]; then
        cp -r $IFCS_SHIM_DIFF_DUMP $out/share/sai-rs/ifcs-shim-scripts
        # the native traces of this library (the golden test above held them to the C's): the aarch64 replay's reference
        ${cc} -std=gnu11 -O1 -g -Wall -Wextra -Werror -rdynamic -o guard_harness ${harnessSrc} -ldl -lpthread
        mkdir -p $out/share/sai-rs/ifcs-shim-traces
        for s in $IFCS_SHIM_DIFF_DUMP/*.txt; do
          rm -rf w; mkdir w
          ./guard_harness $so $s w > $out/share/sai-rs/ifcs-shim-traces/$(basename $s)
        done
      fi
    '';
    meta = old.meta // {
      description = "libsai-teralynx-shim.so in Rust: CPython stubs, Device-memory copy, the learn-ring read guard";
    };
  });

  # aarch64: the differential harness built for the target
  crossHarness = runCommand "ifcs-shim-guard-harness-${arch}" { } ''
    mkdir -p $out/bin
    ${cc} -std=gnu11 -O1 -g -Wall -Wextra -Werror -rdynamic -o $out/bin/guard_harness ${harnessSrc} -ldl -lpthread
  '';

  emulated = runnerPkgs.runCommand "sai-teralynx-shim-rs-${arch}-emulated" { } ''
    set -euo pipefail
    mkdir -p $out/share/sai-rs
    n=0; lines=0
    for s in ${scriptsFrom}/share/sai-rs/ifcs-shim-scripts/*.txt; do
      rm -rf w; mkdir w
      ${crossHarness}/bin/guard_harness ${shim}/lib/libsai-teralynx-shim.so $s w > trace.txt
      ref=${scriptsFrom}/share/sai-rs/ifcs-shim-traces/$(basename $s)
      if ! cmp -s $ref trace.txt; then
        echo "ifcs-shim (${arch}): $(basename $s): the trace differs from the native one"; diff $ref trace.txt | head -40; exit 1
      fi
      if grep -qE 'NOT in do_malloc|NOT ZEROED|WRONG|OTHER' trace.txt; then echo "harness check failed in $s"; exit 1; fi
      n=$((n + 1)); lines=$((lines + $(wc -l < trace.txt)))
    done
    [ $n -gt 0 ] || { echo "ifcs-shim (${arch}): no scripts in ${scriptsFrom}"; exit 1; }
    echo "ifcs-shim (${arch}, emulated): $n scripts, $lines trace lines equal to the native Rust library's (held to the C golden there)" | tee $out/share/sai-rs/differential-${arch}.txt
  '';
in
runCommand "sai-teralynx-shim-rs-${arch}" {
  passthru = { inherit shim; } // lib.optionalAttrs (runnerPkgs != null) { inherit emulated crossHarness; };
} ''
  mkdir -p $out/lib $out/share/sai-rs
  cp ${shim}/lib/libsai-teralynx-shim.so $out/lib/
  cp -r ${shim}/share/sai-rs/. $out/share/sai-rs/
  ${lib.optionalString (runnerPkgs != null) "cp -r ${emulated}/share/sai-rs/. $out/share/sai-rs/"}
''
