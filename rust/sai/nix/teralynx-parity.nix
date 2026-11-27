# The G2 parity harness of the Teralynx backend (crates/sai-teralynx-parity, README "Parity"): the C++
# libsai of the CS8210 (ai_re/nix/packages/sai-teralynx, built as its Makefile does: cppTeralynx) and
# the Rust one (./libsai-teralynx.nix), each on the SAME mock libifcs, driven by the same UNMODIFIED
# programs, compared:
#   checks    test_ifcs (1047 checks): Rust passes N of 1047 (C++: 1047 / 1047), every failure with its area
#   sections  the same runs' IFCS calls (TEST_IFCS_DUMP=1, the tap), test_ifcs section by section
#   driver    the frozen frontend's driver-scripts 01-09 through frontend_driver.cpp, op by op
#   oracle    the oracle's JSON scripts (their SAI steps) through sai_script.cpp, step by step
#   bytes     in every run, the byte tap (c/bytes_rt.c over oracle/gen_tracer.py's wrappers, as
#             SAI_TERALYNX_IFCS_TRACE): ALL bytes of every struct and attribute argument of every IFCS
#             call in the stream, compared with the rest (the mock compares fields, libifcs bytes)
# Checked at build time (native only: it runs them):
#   - progress mode green: every difference allowed (crates/sai-teralynx-parity/tests/parity-allow.txt),
#     pending (its area still has lines in crates/sai-backend-teralynx/tests/test_ifcs-expected.txt, or a
#     tests/parity-pending.txt entry names such an area) or downstream of a pending one; the C++
#     reference passes every check and gives the same output twice; the tap's own check;
#   - the negative control: the Rust side's create_switch IFCS line marked (PARITY_TAP_MUTATE) must
#     make the run red;
#   - the byte negative control: a Rust libsai built with IFCS_POISON_PADDING=1 (the key members' tail
#     padding 0xa5, as the bug left it) must make the run red on an @B line with those bytes, while
#     test_ifcs still passes on it (the mock cannot see padding);
#   - the golden C++ side (crates/sai-teralynx-parity/tests/golden, captured from the C++ libsai at H5-0): the live
#     C++ side must render to its records byte for byte (check-golden.txt: required), the Rust side alone is run
#     against it strictly (golden.txt: reported, required with strict = true) and a golden with a changed digest
#     must make that run red on its section, op block and step (negative-golden.txt: required);
#   - strict mode (the G2 gate) is run and reported, not required (strict = true: required).
# $out/share/sai-rs/teralynx-parity/: report.txt (progress), strict.txt, negative.txt, negative-bytes.txt,
# check-golden.txt, golden.txt, negative-golden.txt, details.txt, gate.txt; $out/bin/sai-teralynx-parity-run: the harness over another Rust libsai (an area owner's
# local run: sai-teralynx-parity-run --rust-lib target/debug/libsai.so [--parts sections --only l2,lag]
# from rust/sai, reads that checkout's tests/ lists).
#   pkgs.callPackage ./teralynx-parity.nix { saiRs = <./package.nix>; cppTeralynx = <sai-teralynx>; }
# aarch64 (the CS8210's architecture; G3): every program cross-built (the fixtures, the C++ and the Rust
# libsai, the harness), the same runs in a derivation of system aarch64-linux (binfmt / qemu-user on the
# build host), the same checks; then its strict report compared with the native one (`reference`, store
# paths masked): an architecture difference is red. Output: share/sai-rs/teralynx-parity/ (+ arch.txt).
#   pkgsCross.aarch64-multiplatform.callPackage ./teralynx-parity.nix { saiRs = <./package.nix for it>;
#     cppTeralynx = <sai-teralynx for it>; runnerPkgs = <nixpkgs, system aarch64-linux>; reference = <native build>; }
{
  lib,
  stdenv,
  callPackage,
  makeWrapper,
  nlohmann_json,
  buildPackages,
  saiRs,
  cppTeralynx,
  strict ? false,
  runnerPkgs ? null,
  reference ? null,
}:
let
  tl = saiRs.teralynxSrc;
  # what the fixtures read of the C++ package: tests (test_ifcs), the shim's C, the frozen
  # frontend's driver and headers; nothing of its library (the mock, the oracle and the scripts: below)
  tlFiles = builtins.path {
    path = tl;
    name = "sai-teralynx-parity-src";
    filter = p: t:
      let
        rel = lib.removePrefix (toString tl + "/") (toString p);
        under = d: lib.hasPrefix (d + "/") rel;
      in
      builtins.elem rel [ "src" "tests" "frontend" "frontend/src" "frontend/tests" ]
      || builtins.elem rel [ "src/shim.c" "src/ifcs_guard.c" ]
      || (under "src" && lib.hasSuffix ".h" rel)
      || under "tests"
      || (under "frontend/src" && lib.hasSuffix ".h" rel)
      || builtins.elem rel [ "frontend/tests/frontend_driver.cpp" "frontend/tests/test_port.h" "frontend/tests/driver_names.awk" ]
      ;
  };
  # the oracle (moved unchanged from the C++ package's oracle/, H5-1): the tracer generator, its runtime header,
  # sai_script.cpp and the scripts
  oracleSrc = builtins.path {
    path = ../../../ai_re/nix/packages/ifcs-oracle;
    name = "ifcs-oracle-parity-src";
    filter = p: t:
      let rel = lib.removePrefix (toString ../../../ai_re/nix/packages/ifcs-oracle + "/") (toString p); in
      rel == "scripts" || lib.hasPrefix "scripts/" rel
      || builtins.elem rel [ "gen_tracer.py" "trace_rt.h" "sai_script.cpp" ];
  };
  # the Teralynx driver scripts 01-09 (moved unchanged from the frontend's tests/driver-scripts, H5-1, to
  # sai-driver's tests/driver-scripts, which also holds the AS7772's 10 and 11)
  driverScripts = lib.fileset.toSource {
    root = ../crates/sai-driver/tests/driver-scripts;
    fileset = lib.fileset.fileFilter (f: f.hasExt "ops" && lib.hasPrefix "0" f.name) ../crates/sai-driver/tests/driver-scripts;
  };
  # the one mock libifcs of every Teralynx test (./teralynx-mock.nix)
  mock = callPackage ./teralynx-mock.nix { inherit (saiRs) ifcsHeaders; };
  crate = ../crates/sai-teralynx-parity;
  lists = ../crates/sai-backend-teralynx/tests;
  rustLib = callPackage ./libsai-teralynx.nix { inherit saiRs cppTeralynx; };
  # the byte negative control's libsai: the key members' padding poisoned (ifcs build.rs,
  # IFCS_POISON_PADDING); a test build only, no checks (crates/ifcs/tests/abi.rs would refuse it)
  rustLibPoisoned = rustLib.overrideAttrs (old: {
    pname = "sai-rs-libsai-teralynx-poisoned";
    env = (old.env or { }) // { IFCS_POISON_PADDING = "1"; };
    doCheck = false;
    postInstall = ''
      if [ -e $out/lib/libsai.so ] && [ ! -L $out/lib/libsai.so ]; then mv $out/lib/libsai.so $out/lib/libsai.so.1.0.0; fi
    '';
  });
  # the harness's program (feature cli: not part of the workspace build's outputs)
  tool = saiRs.overrideAttrs (old: {
    pname = "sai-teralynx-parity-tool";
    cargoBuildFlags = [ "-p" "sai-teralynx-parity" "--features" "cli" ];
    doCheck = false;
    postInstall = "";
    meta = old.meta // { description = "the Teralynx parity harness's program"; };
  });
  cppLib = "${cppTeralynx}/lib/libsai.so.1.0.0";
  canRun = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  arch = stdenv.hostPlatform.parsed.cpu.name;
  # the programs into $fx (CC / CXX / NM: the target's)
  fixturesScript = ''
    MOCK_LIBIFCS=${mock}/mock/libifcs.so ORACLE_SRC=${oracleSrc} PYTHON=${buildPackages.python3}/bin/python3 CC=${stdenv.cc.targetPrefix}cc CXX=${stdenv.cc.targetPrefix}c++ bash ${crate}/build-fixtures.sh $fx ${tlFiles} \
      ${saiRs.ifcsHeaders}/include ${saiRs.saiHeaders} ${cppLib} ${nlohmann_json}/include'';
  # the runs over $fx and the gate (toolBin: the harness's command)
  checksScript = toolBin: ''
    lists() {
      echo --areas ${lists}/areas.txt --expected ${lists}/test_ifcs-expected.txt \
        --allow ${crate}/tests/parity-allow.txt --pending ${crate}/tests/parity-pending.txt \
        --driver-scripts ${driverScripts} --oracle-scripts ${oracleSrc}/scripts
    }
    run() {   # out-name, extra args
      local name=$1; shift
      ${toolBin} --fixtures $fx --work $NIX_BUILD_TOP/w-$name --cpp-lib ${cppLib} \
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
    brc=0; run negative-bytes --parts checks,sections,driver --no-repeat --rust-lib ${rustLibPoisoned}/lib/libsai.so.1.0.0 || brc=$?
    if [ $brc -ne 1 ] || ! grep -q "^== checks: Rust passes 1047 of 1047" negative-bytes.txt \
        || ! grep -q "Rust '@B ifcs_[a-z0-9_]*_set({[0-9a-f]*a5a5a5" negative-bytes.txt; then
      echo "byte negative control: poisoned key padding was NOT reported on an @B line (exit $brc)"; cat negative-bytes.txt; exit 1
    fi
    echo "byte negative control: the poisoned key padding made the run red on its @B lines (test_ifcs still 1047 / 1047), as it must"
    # the golden C++ side (crates/sai-teralynx-parity/tests/golden, captured from the C++ libsai at H5-0): the live C++
    # renders to its records byte for byte (required: the golden is what the C++ gives), and the Rust side alone is
    # judged against it with the same report and strictness (the C++ library is not read)
    gold=${crate}/tests/golden
    ${toolBin} --fixtures $fx --work $NIX_BUILD_TOP/w-check --cpp-lib ${cppLib} $(lists) --check-golden $gold --no-repeat > check-golden.txt 2>&1 || {
      echo "golden: the live C++ side is NOT the golden (a changed C++ library, program or input)"; cat check-golden.txt; exit 1; }
    cat check-golden.txt
    ggrc=0; ${toolBin} --fixtures $fx --work $NIX_BUILD_TOP/w-golden --rust-lib ${rustLib}/lib/libsai.so.1.0.0 $(lists) --golden $gold --strict --no-repeat > golden.txt 2>&1 || ggrc=$?
    sed -n '/^== checks/p; /^== sections/p; /^== driver/p; /^== oracle/p; /^== verdict/p' golden.txt
    ${lib.optionalString strict ''[ $ggrc -eq 0 ] || { echo "parity (golden): RED (exit $ggrc)"; exit 1; }''}
    # its negative control: a changed digest in the golden (a test_ifcs section, a driver op block, an oracle step) is red
    cp -r $gold neg-golden && chmod -R u+w neg-golden
    flip() {   # file, record tag, field with the name, name: the first sha256 of that record with its first digit changed
      awk -F'\t' -v OFS='\t' -v tag="$2" -v col="$3" -v name="$4" '$1==tag && $col==name && !done { c=substr($3,1,1); $3=(c=="0"?"1":"0") substr($3,2); done=1 } { print }' neg-golden/$1 > neg-golden/$1.new
      cmp -s neg-golden/$1 neg-golden/$1.new && { echo "golden negative control: no record $2 '$4' to change"; exit 1; }
      mv neg-golden/$1.new neg-golden/$1
    }
    flip test_ifcs-cpp.txt section 5 l2
    flip driver-cpp.txt block 4 'set switch switch SRC_MAC_ADDRESS=02:00:00:00:aa:01'
    flip oracle-cpp.txt step 4 '0.3 create L3 table (aclorch)'
    ngrc=0; ${toolBin} --fixtures $fx --work $NIX_BUILD_TOP/w-neg-golden --rust-lib ${rustLib}/lib/libsai.so.1.0.0 $(lists) --golden neg-golden --strict --no-repeat > negative-golden.txt 2>&1 || ngrc=$?
    if [ $ngrc -ne 1 ] || ! grep -q "test_ifcs section l2 " negative-golden.txt || ! grep -q "driver 01-switch block" negative-golden.txt \
        || ! grep -q "oracle acl step '0.3 create L3 table (aclorch)'" negative-golden.txt; then
      echo "golden negative control: a changed digest in the golden was NOT reported (exit $ngrc)"; cat negative-golden.txt; exit 1
    fi
    echo "golden negative control: a changed digest in the golden made the run red on its section, op block and step, as it must"
    {
      grep '^== checks' report.txt
      grep '^pending areas' report.txt
      echo "progress mode: exit $rc; strict mode: exit $src ($(grep '^== verdict' strict.txt))"
      echo "golden C++ side: live C++ == golden (check-golden: $(grep '^== verdict' check-golden.txt)); Rust against the golden: exit $ggrc ($(grep '^== verdict' golden.txt))"
    } > gate.txt
    cat gate.txt'';
  # cross: the programs built here, run in ./emulated
  fixtures = stdenv.mkDerivation {
    pname = "sai-teralynx-parity-fixtures-${arch}";
    inherit (saiRs) version;
    dontUnpack = true;
    dontConfigure = true;
    # built where the native build builds them (no store -L path: no RUNPATH; LD_LIBRARY_PATH picks the libsai);
    # build-fixtures.sh's plain nm: the target's
    buildPhase = ''
      runHook preBuild
      mkdir -p nm-bin && ln -s ${stdenv.cc.bintools.bintools}/bin/${stdenv.cc.targetPrefix}nm nm-bin/nm
      export PATH=$PWD/nm-bin:$PATH
      fx=$PWD/fixtures
      ${fixturesScript}
      runHook postBuild
    '';
    installPhase = "mkdir -p $out && cp -r fixtures/bin fixtures/mock fixtures/cfg $out/";
    dontFixup = true;
  };
  # the strict report of a run, store paths masked (the libraries' paths differ per architecture)
  masked = f: "sed -E 's,/nix/store/[a-z0-9]{32}-[^/ ]*,<store>,g' ${f}";
  emulated = runnerPkgs.runCommand "sai-teralynx-parity-${arch}" { } ''
    set -euo pipefail
    fx=$PWD/fixtures
    cp -r ${fixtures} $fx
    chmod -R u+w $fx
    ${checksScript "${tool}/bin/sai-teralynx-parity --timeout 3600"}
    uname -m > machine
    ${lib.optionalString (reference != null) ''
      ${masked "${reference}/share/sai-rs/teralynx-parity/strict.txt"} > strict-ref.txt
      ${masked "strict.txt"} > strict-here.txt
      if diff -u strict-ref.txt strict-here.txt > arch.txt; then
        echo "strict report on $(cat machine) (emulated) equals the native one (${reference}), store paths masked" > arch.txt
        cat arch.txt
      else
        echo "parity: the $(cat machine) strict report differs from the native one:"; cat arch.txt; exit 1
      fi
    ''}
    d=$out/share/sai-rs/teralynx-parity
    mkdir -p $d
    cp report.txt strict.txt negative.txt negative-bytes.txt check-golden.txt golden.txt negative-golden.txt details.txt gate.txt machine $d/
    ${lib.optionalString (reference != null) "cp arch.txt $d/"}
  '';
in
if !canRun then
  assert runnerPkgs != null;
  emulated.overrideAttrs (old: { passthru = { inherit tool rustLib rustLibPoisoned tlFiles oracleSrc driverScripts mock fixtures; }; })
else
stdenv.mkDerivation {
  pname = "sai-teralynx-parity";
  inherit (saiRs) version;
  dontUnpack = true;
  dontConfigure = true;
  nativeBuildInputs = [ makeWrapper ];

  buildPhase = ''
    runHook preBuild
    fx=$PWD/fixtures
    ${fixturesScript}
    ${checksScript "${tool}/bin/sai-teralynx-parity"}
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    d=$out/share/sai-rs/teralynx-parity
    mkdir -p $d $out/bin $out/libexec/sai-teralynx-parity
    cp report.txt strict.txt negative.txt negative-bytes.txt check-golden.txt golden.txt negative-golden.txt details.txt gate.txt $d/
    cp -r fixtures/bin fixtures/mock fixtures/cfg $out/libexec/sai-teralynx-parity/
    cp ${tool}/bin/sai-teralynx-parity $out/bin/
    # an area owner's run from a checkout of rust/sai: that tree's lists, the store's fixtures and C++
    makeWrapper $out/bin/sai-teralynx-parity $out/bin/sai-teralynx-parity-run \
      --add-flags "--fixtures $out/libexec/sai-teralynx-parity --cpp-lib ${cppLib}" \
      --add-flags "--driver-scripts ${driverScripts} --oracle-scripts ${oracleSrc}/scripts" \
      --run 'if [ -f crates/sai-teralynx-parity/Cargo.toml ]; then set -- --areas crates/sai-backend-teralynx/tests/areas.txt --expected crates/sai-backend-teralynx/tests/test_ifcs-expected.txt --allow crates/sai-teralynx-parity/tests/parity-allow.txt --pending crates/sai-teralynx-parity/tests/parity-pending.txt "$@"; else echo "sai-teralynx-parity-run: run it from rust/sai (the lists are read from the checkout)" >&2; exit 2; fi' \
      --run 'case " $* " in *" --work "*) ;; *) set -- --work "''${SAI_RS_TEST_TMPDIR:-$PWD/target/teralynx-parity}" "$@" ;; esac'
    runHook postInstall
  '';

  dontStrip = true;
  passthru = { inherit tool rustLib rustLibPoisoned tlFiles oracleSrc driverScripts mock; };

  meta = {
    description = "Parity of the Rust and the C++ Teralynx libsai on the mock libifcs (test_ifcs, call streams, driver and oracle scripts)";
    license = lib.licenses.asl20;
    platforms = [ "x86_64-linux" ];
  };
}