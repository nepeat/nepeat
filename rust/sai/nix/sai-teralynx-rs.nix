# The CS8210's SAI package in Rust, the drop-in for the C++ one it replaced (the sai-teralynx package,
# deleted in H5 on 2026-11-27, last in the tree at commit 767d9b0): the same output layout, every file a Rust
# build (README "Teralynx tools", "Drop-in package"):
#   lib/libsai.so -> libsai.so.1 -> libsai.so.1.0.0   sai-ffi + sai-backend-teralynx (./libsai-teralynx.nix: its checks)
#   lib/libsai-teralynx-shim.so    crates ifcs-shim + ifcs-guard (./ifcs-shim.nix: its checks, the C golden)
#   bin/openbcm-diag               crate openbcm-diag (the diag socket client; the workspace build's openbcm-diag-rs)
#   bin/sai-teralynx-smoke         crate sai-teralynx-smoke: the C++ program's form (sai_smoke.cpp) + the H1 gate
#   bin/sai-script                 crate sai-script: the oracle's JSON script runner (the C++ runner sai_script.cpp's
#                                  form; the C++ package did not install it)
#   share/sai-rs/                  the parts' reports, drop-in.txt (this file's checks)
#
#   native (x86_64: the mock checks run):
#     pkgs.callPackage ./sai-teralynx-rs.nix { saiRs = <./package.nix>; }
#   aarch64 (the CS8210): pkgsCross.aarch64-multiplatform.callPackage ./sai-teralynx-rs.nix { saiRs = ...; }
# The C++ package's file list and exports, recorded when it was deleted: ./golden/sai-teralynx-package.txt.
#
# Checked (a failure fails the build):
#   - always: every file and link the C++ package installed has one here; libsai.so.1.0.0 exports exactly the
#     17 symbols of the virtual-backend library (the C++ one's 15 among them; +sai_bulk_get_attribute,
#     +sai_query_object_stage); soname libsai.so.1
#   - native (the tools' build): fmt, clippy -D warnings and the tests of sai-client, sai-script and
#     sai-teralynx-smoke; sai-script against the golden of the C++ runner (crates/sai-script/tests/differential.rs,
#     tests/golden/sai-script-cpp.txt) on the mock through the Rust libsai: --cpp-names identical on every oracle
#     script, the fixed mode identical but on tests/fixed-scripts.txt's scripts, which run to their end; the exec
#     interface (--tool); the smoke program on the mock (crates/sai-teralynx-smoke/tests/mock-smoke.sh) with
#     this package's lib/: the gate's full run, --recreate, --hold, the failure paths and refusals, the C++
#     program's form identical to its golden (tests/golden/smoke-cpp.txt); the mock test hooks of sai-client
#     (feature mock) are in no shipped program (nm)
{
  lib,
  stdenv,
  callPackage,
  makeWrapper,
  saiRs,
}:
let
  canRun = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  bin = "${stdenv.cc.bintools.bintools}/bin/${stdenv.cc.targetPrefix}";
  triple = stdenv.hostPlatform.rust.rustcTarget;
  libsai = callPackage ./libsai-teralynx.nix { inherit saiRs; };
  shim = callPackage ./ifcs-shim.nix { inherit saiRs; };
  # the oracle fixtures (mock libifcs, Rust shim, tap, oracle-prep, port maps) and the scripts, as the parity
  # harness built them (natively only)
  parity = callPackage ./teralynx-parity.nix { inherit saiRs; };
  golden = ./golden/sai-teralynx-package.txt;
  fixtures = "${parity}/libexec/sai-teralynx-parity";
  crates = [ "sai-client" "sai-script" "sai-teralynx-smoke" ];
  pkgFlags = lib.concatMap (c: [ "-p" c ]) crates;
  tools = saiRs.overrideAttrs (old: {
    pname = "sai-teralynx-tools";
    cargoBuildFlags = [ "-p" "sai-script" "-p" "sai-teralynx-smoke" "--features" "cli" ];
    cargoTestFlags = pkgFlags;
    # only what these crates need: not the board tables the workspace build's goldens read
    env = builtins.removeAttrs old.env [ "PHY_SEQ_TABLES" ];
    doCheck = canRun;
    preCheck = lib.optionalString canRun ''
      cargo fmt ${lib.concatStringsSep " " pkgFlags} --check
      cargo clippy --offline ${lib.concatStringsSep " " pkgFlags} --all-targets --features cli --profile "$cargoCheckType" -- -D warnings
      export SAI_RS_TEST_TMPDIR=$NIX_BUILD_TOP/t; mkdir -p $SAI_RS_TEST_TMPDIR
      export SAI_SCRIPT_DIFF_REQUIRE=1 SAI_SCRIPT_DIFF_FIXTURES=${fixtures} SAI_SCRIPT_DIFF_SCRIPTS=${parity.oracleSrc}/scripts
      export SAI_SCRIPT_DIFF_LIBS=rust=${libsai}/lib/libsai.so.1.0.0
      export SAI_SCRIPT_DIFF_RUNNER=$PWD/target/${triple}/$cargoBuildType/sai-script
      # the tests' summary lines (the sai-script golden comparison's counts among them)
      mkdir -p $NIX_BUILD_TOP/sai-script-diff
      export SAI_RS_TEST_SUMMARY=$NIX_BUILD_TOP/sai-script-diff/report.txt
      test -x $SAI_SCRIPT_DIFF_RUNNER
    '';
    postCheck = lib.optionalString canRun ''
      lib=$NIX_BUILD_TOP/pkg-lib; mkdir -p $lib
      cp ${libsai}/lib/libsai.so.1.0.0 ${shim}/lib/libsai-teralynx-shim.so $lib/
      ln -s libsai.so.1.0.0 $lib/libsai.so.1
      CC=${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc bash crates/sai-teralynx-smoke/tests/mock-smoke.sh $NIX_BUILD_TOP/mock-smoke \
        $PWD/target/${triple}/$cargoBuildType/sai-teralynx-smoke $lib ${fixtures} crates/sai-backend-teralynx/tests \
        | tee $NIX_BUILD_TOP/mock-smoke.txt
      [ "''${PIPESTATUS[0]}" -eq 0 ] || { echo "mock-smoke: red"; exit 1; }
    '';
    postInstall = ''
      mkdir -p $out/share/sai-rs
    '' + lib.optionalString canRun ''
      cp $NIX_BUILD_TOP/mock-smoke.txt $out/share/sai-rs/smoke-mock.txt
      grep -q . $NIX_BUILD_TOP/sai-script-diff/report.txt
      cp $NIX_BUILD_TOP/sai-script-diff/report.txt $out/share/sai-rs/sai-script-differential.txt
    '';
    meta = old.meta // { description = "sai-teralynx-smoke and sai-script (Rust) for the CS8210"; };
  });
in
stdenv.mkDerivation {
  pname = "sai-teralynx-rs";
  inherit (saiRs) version;
  dontUnpack = true;
  dontConfigure = true;
  dontBuild = true;
  # copies of finished binaries
  dontFixup = true;
  nativeBuildInputs = [ makeWrapper ];

  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib $out/bin $out/share/sai-rs
    cp ${libsai}/lib/libsai.so.1.0.0 $out/lib/
    ln -s libsai.so.1.0.0 $out/lib/libsai.so.1
    ln -s libsai.so.1 $out/lib/libsai.so
    cp ${shim}/lib/libsai-teralynx-shim.so $out/lib/
    install -m755 ${saiRs}/bin/openbcm-diag-rs $out/bin/openbcm-diag
    # the two tools dlopen the vendor libifcs.so outside syncd, which needs libstdc++.so.6 (the Rust libsai does
    # not link it): wrapped with the HOST platform's libstdc++ on LD_LIBRARY_PATH; the real programs sit in
    # libexec/ so that "lib/ beside bin/" (sai-client's default library) still finds $out/lib
    mkdir -p $out/libexec
    for t in sai-teralynx-smoke sai-script; do
      install -m755 ${tools}/bin/$t $out/libexec/$t
      makeWrapper $out/libexec/$t $out/bin/$t --prefix LD_LIBRARY_PATH : ${stdenv.cc.cc.lib}/lib
    done
    cp -r ${libsai}/share/sai-rs/. ${tools}/share/sai-rs/. $out/share/sai-rs/
    cp ${shim}/share/sai-rs/*.txt $out/share/sai-rs/
    chmod -R u+w $out/share/sai-rs

    report=$out/share/sai-rs/drop-in.txt
    exports() { ${bin}nm -D --defined-only "$1" | awk '{ print $3 }' | LC_ALL=C sort; }
    exports $out/lib/libsai.so.1.0.0 > exports.rust
    section() { awk -v want="== $1" '/^== / { on = ($0 == want); next } on && NF' ${golden}; }
    section "libsai.so.1.0.0 exports" > exports.cpp
    exports ${saiRs}/lib/libsai.so.1.0.0 > exports.virtual
    section files > files.cpp
    test -s exports.cpp && test -s files.cpp
    (cd $out && find . -mindepth 1 \( -type f -o -type l \) -not -path './share/*' | LC_ALL=C sort) > files.rust
    {
      echo "sai-teralynx-rs: the Rust drop-in of the C++ sai-teralynx (its files and exports: ${golden}), ${stdenv.hostPlatform.config}"
      echo "== files (C++ package -> here)"
      while read -r f; do
        if [ -e "$out/$f" ] || [ -L "$out/$f" ]; then echo "$f  ok"; else echo "$f  MISSING"; fi
      done < files.cpp
      echo "== only here"
      comm -13 files.cpp files.rust
      echo "== libsai.so.1.0.0 exports: $(wc -l < exports.rust) (the virtual-backend library: $(wc -l < exports.virtual), the C++: $(wc -l < exports.cpp))"
      echo "not in the C++ library: $(comm -23 exports.rust exports.cpp | tr '\n' ' ')"
      ${bin}readelf -d $out/lib/libsai.so.1.0.0 | grep -E 'SONAME|NEEDED'
      ${bin}readelf -d $out/lib/libsai-teralynx-shim.so | grep -E 'SONAME'
    } > $report
    cat $report
    if grep -q '  MISSING$' $report; then echo "drop-in: a file of the C++ package has no counterpart"; exit 1; fi
    diff -u exports.virtual exports.rust || { echo "drop-in: exports differ from the virtual-backend library"; exit 1; }
    [ "$(wc -l < exports.rust)" -eq 17 ] || { echo "drop-in: not 17 exports"; exit 1; }
    if [ -n "$(comm -13 exports.rust exports.cpp)" ]; then echo "drop-in: the C++ library exports more"; exit 1; fi
    ${bin}readelf -d $out/lib/libsai.so.1.0.0 | grep -q 'SONAME.*\[libsai.so.1\]' || { echo "drop-in: wrong soname"; exit 1; }
    ${bin}readelf -d $out/lib/libsai-teralynx-shim.so | grep -q 'SONAME.*\[libsai-teralynx-shim.so\]' || { echo "drop-in: wrong shim soname"; exit 1; }
    # sai-client's mock test hooks (feature mock: the end-to-end test only) are in no shipped program
    for t in sai-teralynx-smoke sai-script; do
      if grep -a -q -E 'mock_(fire_link|ifcs_log_clear|learn_pass|reg_set)' $out/libexec/$t; then
        echo "drop-in: libexec/$t carries the mock libifcs's test hooks (sai-client feature mock)"; exit 1
      fi
      echo "libexec/$t: no mock test hooks (sai-client without feature mock)" >> $report
    done
    # the wrappers' LD_LIBRARY_PATH holds a libstdc++.so.6 of the host platform's machine type
    machine() { ${bin}readelf -h "$1" | awk -F: '/Machine:/ { gsub(/^ +/, "", $2); print $2 }'; }
    want=$(machine $out/lib/libsai.so.1.0.0)
    for t in sai-teralynx-smoke sai-script; do
      dir=$(grep -o '/nix/store/[^:" ]*-lib/lib' $out/bin/$t | grep -F gcc | head -n1 || true)
      [ -n "$dir" ] && [ -e "$dir/libstdc++.so.6" ] || { echo "drop-in: bin/$t: no libstdc++.so.6 on its LD_LIBRARY_PATH ($dir)"; exit 1; }
      [ "$(machine "$dir/libstdc++.so.6")" = "$want" ] || { echo "drop-in: bin/$t: libstdc++.so.6 is not for $want"; exit 1; }
      echo "bin/$t: LD_LIBRARY_PATH $dir (libstdc++.so.6, $want)" >> $report
    done
    runHook postInstall
  '';

  passthru = {
    inherit libsai shim tools;
    inherit (saiRs) saiHeaders;
  };

  meta = {
    description = "The Rust SAI of the CS8210-32X (libsai, shim, diag client, smoke program, oracle runner) under sai-teralynx's names";
    license = lib.licenses.asl20;
    platforms = [ "x86_64-linux" "aarch64-linux" ];
  };
}
