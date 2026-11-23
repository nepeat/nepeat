# The workspace's test suites built FOR a target and packed to run ON it (no compiler, no
# cargo, no Nix store needed there): what "cargo test" would run, as a directory.
#   pkgsCross.callPackage ./target-tests.nix { saiRs = <./package.nix for the same pkgs>; }
# Why: a cross build cannot execute its tests, and the Rust SAI's first target is big-endian
# ppc64 (AS7760 / AS7772), where byte order in unions, lists and MDIO words is exactly what
# an x86 test run cannot show. README, "Big-endian runs".
#
# $out/
#   tests/<name>-<hash>   every test executable of the workspace (cargo test --no-run, the
#                         test profile: debug assertions and overflow checks on)
#   lib/libsai.so         the cdylib of that same build; lib-release/libsai.so the one the
#                         package ships (the C harness runs against both)
#   bin/harness           crates/sai-ffi/tests/harness.c, built with the target's C compiler
#   bin/...               the workspace's own binaries as the tests build them (the MDIO tool on
#                         the Rust fake of the glue, the phy-seq tools) and C/C++ helpers crates prebuilt
#                         in their build scripts (OUT_DIR/target-tests): the frozen C++ frontend's
#                         drivers and C client the Rust ones are compared with live (with the
#                         package's cppFrontendSrc); the comparisons with deleted C / C++ are
#                         goldens compiled into the test executables
#   sysroot/              the target's loader, libc, libgcc_s: the bundle runs on a machine
#                         whose own glibc is another version or lives elsewhere
#                         (run.sh probes each helper by starting it without arguments: a helper
#                         must be harmless when run that way)
#   run.sh                runs everything, prints one line per suite, exits non-zero on a failure
# passthru.runOn: "sai-rs-test-on <ssh host> [ssh command...]" copies the bundle to a scratch
# directory under /tmp of the host, runs run.sh there and removes the directory.
# Touches nothing else on the host: no chip, no SDK, no service (the virtual backend refuses
# to start next to /dev/linux-*-bde unless told; run.sh tells it, see there).
# macsecTable, extPhyTable: optional, a board's external MACsec / PHY tables (the AS7772's
# generated ones): the end-to-end MACsec comparison and phy-seq's board-table goldens then also
# run over them on the target.
{ lib, stdenv, saiRs, buildPackages, macsecTable ? null, extPhyTable ? null }:
let
  cc = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
  triple = stdenv.hostPlatform.rust.rustcTarget;
  bundle = saiRs.overrideAttrs (old: {
    pname = "sai-rs-target-tests";
    # The build phase makes the release libsai.so as usual. Then the test executables: by
    # nixpkgs' check hook (the cross flags and linker settings live in it), called here
    # because a cross build has no check phase; --no-run, nothing is executed.
    doCheck = false;
    cargoCheckType = "debug";
    cargoTestFlags = [ "--workspace" "--no-run" ];
    preCheck = "";
    dontStrip = true;   # test executables with symbols: a failure on the target has a backtrace
    postBuild = ''
      cargoCheckHook
      dbg=target/${triple}/debug
      mkdir -p $out/tests $out/lib $out/lib-release $out/bin $out/sysroot
      # which executables are tests: cargo says (a second, already built, pass for its messages)
      cargoTestFlags="$cargoTestFlags --message-format=json" cargoCheckHook > cargo-messages.txt
      grep '^{' cargo-messages.txt \
        | ${buildPackages.jq}/bin/jq -r 'select(.reason == "compiler-artifact" and .executable != null) | (if .profile.test then "tests" else "bin" end) + " " + .executable' \
        | while read -r kind exe; do cp "$exe" $out/$kind/; done
      cp $dbg/deps/libsai.so $out/lib/libsai.so
      cp target/${triple}/release/libsai.so $out/lib-release/libsai.so
      ${cc} -std=gnu11 -Wall -Wextra -Werror -O1 -g -I ${saiRs.saiHeaders} -I ${saiRs.saiHeaders}/../experimental \
        crates/sai-ffi/tests/harness.c -o $out/bin/harness -ldl -lpthread
      ${lib.optionalString (macsecTable != null) "cp ${macsecTable} $out/ext-macsec.tbl"}
      ${lib.optionalString (extPhyTable != null) "cp ${extPhyTable} $out/ext-phy.tbl"}
      # crates that prebuild C test helpers leave them in OUT_DIR/target-tests
      for extra in $dbg/build/*/out/target-tests; do
        if [ -d "$extra" ]; then cp -rn "$extra"/. $out/bin/; fi
      done
      cp -L ${stdenv.cc.libc}/lib/ld*.so.* $out/sysroot/
      for l in libc.so.6 libm.so.6 libdl.so.2 libpthread.so.0 librt.so.1; do
        cp -L ${stdenv.cc.libc}/lib/$l $out/sysroot/
      done
      cp -L $(${cc} -print-file-name=libgcc_s.so.1) $out/sysroot/
      cp -L $(${cc} -print-file-name=libstdc++.so.6) $out/sysroot/   # C++ helpers (sai-macsec's driver)
      cp ${./target-tests-run.sh} $out/run.sh
      chmod +x $out/run.sh
      ls $out/tests | sed 's/-[0-9a-f]*$//' | sort > $out/SUITES
      echo "${stdenv.hostPlatform.config}" > $out/TARGET
    '';
    installPhase = "runHook preInstall; test -s $out/SUITES; runHook postInstall";
    postInstall = "";
    dontFixup = true;    # no patchelf, no shebang rewriting: it runs outside a store
    meta = old.meta // { description = "sai-rs test executables for ${stdenv.hostPlatform.config}, to run on the target"; };
  });
  runOn = buildPackages.writeShellApplication {
    name = "sai-rs-test-on";
    text = ''
      if [ $# -lt 1 ]; then
        echo "usage: sai-rs-test-on <host> [ssh command, default: ssh]   (e.g. sai-rs-test-on root@10.60.2.23 sudo ssh)" >&2
        exit 2
      fi
      host=$1; shift
      if [ $# -eq 0 ]; then set -- ssh; fi
      echo "sai-rs tests for $(cat ${bundle}/TARGET) on $host (bundle ${bundle})" >&2
      # one connection: unpack to a new scratch directory, run, remove it whatever happened
      # shellcheck disable=SC2016
      tar -C ${bundle} -cf - . | "$@" -o BatchMode=yes "$host" '
        d=$(mktemp -d /tmp/sai-rs-tests.XXXXXX) || exit 97
        trap "rm -rf $d" EXIT
        tar -C "$d" -xf - || exit 98
        chmod -R u+w "$d"
        "$d/run.sh"'
    '';
  };
in
bundle.overrideAttrs (old: { passthru = (old.passthru or { }) // { inherit runOn; }; })
