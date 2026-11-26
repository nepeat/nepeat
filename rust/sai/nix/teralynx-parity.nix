# The G2 parity harness of the Teralynx backend (crates/sai-teralynx-parity, README "Parity"): the C++
# libsai of the CS8210 (ai_re/nix/packages/sai-teralynx, built as its Makefile does: cppTeralynx) and
# the Rust one (./libsai-teralynx.nix), each on the SAME mock libifcs, driven by the same UNMODIFIED
# programs, compared:
#   checks    test_ifcs (1047 checks): Rust passes N of 1047 (C++: 1047 / 1047), every failure with its area
#   sections  the same runs' IFCS calls (TEST_IFCS_DUMP=1, the tap), test_ifcs section by section
#   driver    the frozen frontend's driver-scripts 01-09 through frontend_driver.cpp, op by op
#   oracle    the oracle's JSON scripts (their SAI steps) through sai_script.cpp, step by step
# Checked at build time (native only: it runs them):
#   - progress mode green: every difference allowed (crates/sai-teralynx-parity/tests/parity-allow.txt),
#     pending (its area still has lines in crates/sai-backend-teralynx/tests/test_ifcs-expected.txt, or a
#     tests/parity-pending.txt entry names such an area) or downstream of a pending one; the C++
#     reference passes every check and gives the same output twice; the tap's own check;
#   - the negative control: the Rust side's create_switch IFCS line marked (PARITY_TAP_MUTATE) must
#     make the run red;
#   - strict mode (the G2 gate) is run and reported, not required (strict = true: required).
# $out/share/sai-rs/teralynx-parity/: report.txt (progress), strict.txt, negative.txt, details.txt,
# gate.txt; $out/bin/sai-teralynx-parity-run: the harness over another Rust libsai (an area owner's
# local run: sai-teralynx-parity-run --rust-lib target/debug/libsai.so [--parts sections --only l2,lag]
# from rust/sai, reads that checkout's tests/ lists).
#   pkgs.callPackage ./teralynx-parity.nix { saiRs = <./package.nix>; cppTeralynx = <sai-teralynx>; }
{
  lib,
  stdenv,
  callPackage,
  makeWrapper,
  nlohmann_json,
  saiRs,
  cppTeralynx,
  strict ? false,
}:
let
  tl = saiRs.teralynxSrc;
  # what the fixtures read of the C++ package: tests (test_ifcs, the mock), the shim's C, the frozen
  # frontend's driver and headers, the oracle's runner and scripts; nothing of its library
  tlFiles = builtins.path {
    path = tl;
    name = "sai-teralynx-parity-src";
    filter = p: t:
      let
        rel = lib.removePrefix (toString tl + "/") (toString p);
        under = d: lib.hasPrefix (d + "/") rel;
      in
      builtins.elem rel [ "src" "tests" "frontend" "frontend/src" "frontend/tests" "frontend/tests/driver-scripts" "oracle" "oracle/scripts" ]
      || builtins.elem rel [ "src/shim.c" "src/ifcs_guard.c" "oracle/sai_script.cpp" ]
      || (under "src" && lib.hasSuffix ".h" rel)
      || under "tests"
      || (under "frontend/src" && lib.hasSuffix ".h" rel)
      || builtins.elem rel [ "frontend/tests/frontend_driver.cpp" "frontend/tests/test_port.h" "frontend/tests/driver_names.awk" ]
      || under "frontend/tests/driver-scripts"
      || under "oracle/scripts";
  };
  crate = ../crates/sai-teralynx-parity;
  lists = ../crates/sai-backend-teralynx/tests;
  rustLib = callPackage ./libsai-teralynx.nix { inherit saiRs cppTeralynx; };
  # the harness's program (feature cli: not part of the workspace build's outputs)
  tool = saiRs.overrideAttrs (old: {
    pname = "sai-teralynx-parity-tool";
    cargoBuildFlags = [ "-p" "sai-teralynx-parity" "--features" "cli" ];
    doCheck = false;
    postInstall = "";
    meta = old.meta // { description = "the Teralynx parity harness's program"; };
  });
  cppLib = "${cppTeralynx}/lib/libsai.so.1.0.0";
in
stdenv.mkDerivation {
  pname = "sai-teralynx-parity";
  inherit (saiRs) version;
  dontUnpack = true;
  dontConfigure = true;
  nativeBuildInputs = [ makeWrapper ];

  buildPhase = ''
    runHook preBuild
    fx=$PWD/fixtures
    CC=${stdenv.cc.targetPrefix}cc CXX=${stdenv.cc.targetPrefix}c++ bash ${crate}/build-fixtures.sh $fx ${tlFiles} \
      ${saiRs.ifcsHeaders}/include ${saiRs.saiHeaders} ${cppLib} ${nlohmann_json}/include
    lists() {
      echo --areas ${lists}/areas.txt --expected ${lists}/test_ifcs-expected.txt \
        --allow ${crate}/tests/parity-allow.txt --pending ${crate}/tests/parity-pending.txt \
        --driver-scripts ${tlFiles}/frontend/tests/driver-scripts --oracle-scripts ${tlFiles}/oracle/scripts
    }
    run() {   # out-name, extra args
      local name=$1; shift
      ${tool}/bin/sai-teralynx-parity --fixtures $fx --work $NIX_BUILD_TOP/w-$name --cpp-lib ${cppLib} \
        --rust-lib ${rustLib}/lib/libsai.so.1.0.0 $(lists) "$@" > $name.txt 2>&1
    }
    rc=0; run report || rc=$?
    cat report.txt
    cp $NIX_BUILD_TOP/w-report/out/details.txt details.txt
    [ $rc -eq 0 ] || ${if strict then "{ echo 'parity: red (strict build: the strict run decides)'; }" else "{ echo \"parity: RED (exit $rc)\"; exit 1; }"}
    src=0; run strict --strict --no-repeat || src=$?
    sed -n '/^== checks/p; /^== sections/p; /^== driver/p; /^== oracle/p; /^== verdict/p' strict.txt
    ${lib.optionalString strict ''[ $src -eq 0 ] || { echo "parity (strict): RED (exit $src)"; exit 1; }''}
    nrc=0; run negative --parts sections,driver --no-repeat --mutate ifcs_node_create || nrc=$?
    if [ $nrc -ne 1 ] || ! grep -q 'ifcs_node_create node=0 count=0 attrs=null \[mutated\]' negative.txt; then
      echo "negative control: a changed Rust call line was NOT reported (exit $nrc)"; cat negative.txt; exit 1
    fi
    echo "negative control: the changed Rust call line made the run red, as it must"
    {
      grep '^== checks' report.txt
      grep '^pending areas' report.txt
      echo "progress mode: exit $rc; strict mode: exit $src ($(grep '^== verdict' strict.txt))"
    } > gate.txt
    cat gate.txt
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    d=$out/share/sai-rs/teralynx-parity
    mkdir -p $d $out/bin $out/libexec/sai-teralynx-parity
    cp report.txt strict.txt negative.txt details.txt gate.txt $d/
    cp -r fixtures/bin fixtures/mock fixtures/cfg $out/libexec/sai-teralynx-parity/
    cp ${tool}/bin/sai-teralynx-parity $out/bin/
    # an area owner's run from a checkout of rust/sai: that tree's lists, the store's fixtures and C++
    makeWrapper $out/bin/sai-teralynx-parity $out/bin/sai-teralynx-parity-run \
      --add-flags "--fixtures $out/libexec/sai-teralynx-parity --cpp-lib ${cppLib}" \
      --add-flags "--driver-scripts ${tlFiles}/frontend/tests/driver-scripts --oracle-scripts ${tlFiles}/oracle/scripts" \
      --run 'if [ -f crates/sai-teralynx-parity/Cargo.toml ]; then set -- --areas crates/sai-backend-teralynx/tests/areas.txt --expected crates/sai-backend-teralynx/tests/test_ifcs-expected.txt --allow crates/sai-teralynx-parity/tests/parity-allow.txt --pending crates/sai-teralynx-parity/tests/parity-pending.txt "$@"; else echo "sai-teralynx-parity-run: run it from rust/sai (the lists are read from the checkout)" >&2; exit 2; fi' \
      --run 'case " $* " in *" --work "*) ;; *) set -- --work "''${SAI_RS_TEST_TMPDIR:-$PWD/target/teralynx-parity}" "$@" ;; esac'
    runHook postInstall
  '';

  dontStrip = true;
  passthru = { inherit tool rustLib tlFiles; };

  meta = {
    description = "Parity of the Rust and the C++ Teralynx libsai on the mock libifcs (test_ifcs, call streams, driver and oracle scripts)";
    license = lib.licenses.asl20;
    platforms = [ "x86_64-linux" ];
  };
}
