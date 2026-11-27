# The parity harness of the Teralynx backend (crates/sai-teralynx-parity, README "Parity"): the Rust libsai of
# the CS8210 (./libsai-teralynx.nix) on the mock libifcs, judged against the golden C++ side
# (crates/sai-teralynx-parity/tests/golden: what the C++ libsai of the C++ sai-teralynx package gave on the
# same mock for the same programs, captured at H5-0; the C++ package was deleted in H5 on 2026-11-27, last in
# the tree at commit 767d9b0). Parts:
#   checks    the Rust test_ifcs (crates/sai-backend-teralynx/tests/ifcs_e2e, the port of the C++ test_ifcs):
#             Rust passes N of 1047, per section, every failure with its area
#   sections  the same run's IFCS calls (TEST_IFCS_DUMP=1, the tap), test_ifcs section by section
#   oracle    the oracle's JSON scripts (their SAI steps) through the Rust sai-script --cpp-names, step by step
#   bytes     in every run, the byte tap (c/bytes_rt.c over ifcs-oracle's gen_tracer.py wrappers, as
#             SAI_TERALYNX_IFCS_TRACE): ALL bytes of every struct and attribute argument of every IFCS
#             call in the stream, compared with the rest (the mock compares fields, libifcs bytes)
# The driver part (the op scripts through the C++ frontend_driver.cpp on the libsai) has no program since the
# frozen frontend was deleted (H5): its golden stays, checked by the crate's tests (tests/golden.rs), not run here.
# Checked at build time (native only: it runs them):
#   - progress mode green: every difference allowed (crates/sai-teralynx-parity/tests/parity-allow.txt),
#     pending (its area still has lines in crates/sai-backend-teralynx/tests/test_ifcs-expected.txt, or a
#     tests/parity-pending.txt entry names such an area) or downstream of a pending one;
#   - the negative control: the Rust side's create_switch IFCS line marked (PARITY_TAP_MUTATE) must
#     make the run red;
#   - the byte negative control: a Rust libsai built with IFCS_POISON_PADDING=1 (the key members' tail
#     padding 0xa5, as the bug left it) must make the run red on its sections, an @B line with those bytes,
#     while test_ifcs still passes on it (the mock cannot see padding);
#   - the golden negative control: a golden with a changed digest (a test_ifcs section, an oracle step) must
#     make the run red on that section and step;
#   - strict mode (the G2 gate) is run and reported, not required (strict = true: required).
# $out/share/sai-rs/teralynx-parity/: report.txt (progress), strict.txt, negative.txt, negative-bytes.txt,
# negative-golden.txt, details.txt, gate.txt; $out/bin/sai-teralynx-parity-run: the harness over another Rust
# libsai (an area owner's local run: sai-teralynx-parity-run --rust-lib target/debug/libsai.so [--parts sections
# --only l2,lag] from rust/sai, reads that checkout's tests/ lists and goldens).
#   pkgs.callPackage ./teralynx-parity.nix { saiRs = <./package.nix>; }
# aarch64 (the CS8210's architecture; G3): every program cross-built (the fixtures, the Rust libsai, the Rust
# test_ifcs, the harness), the same runs in a derivation of system aarch64-linux (binfmt / qemu-user on the
# build host), the same checks; then its strict report compared with the native one (`reference`, store
# paths masked): an architecture difference is red. Output: share/sai-rs/teralynx-parity/ (+ arch.txt).
#   pkgsCross.aarch64-multiplatform.callPackage ./teralynx-parity.nix { saiRs = <./package.nix for it>;
#     runnerPkgs = <nixpkgs, system aarch64-linux>; reference = <native build>; }
{
  lib,
  stdenv,
  callPackage,
  makeWrapper,
  nlohmann_json,
  buildPackages,
  saiRs,
  strict ? false,
  runnerPkgs ? null,
  reference ? null,
}:
let
  # the oracle (moved unchanged from the C++ package's oracle/, H5-1): the tracer generator, its runtime header
  # and the scripts
  oracleDir = ../../../ai_re/nix/packages/ifcs-oracle;
  oracleSrc = builtins.path {
    path = oracleDir;
    name = "ifcs-oracle-parity-src";
    filter = p: t:
      let rel = lib.removePrefix (toString oracleDir + "/") (toString p); in
      rel == "scripts" || lib.hasPrefix "scripts/" rel
      || builtins.elem rel [ "gen_tracer.py" "trace_rt.h" ];
  };
  # the Teralynx driver scripts 01-09 (moved unchanged from the frontend's tests/driver-scripts, H5-1, to
  # sai-driver's tests/driver-scripts): the driver part's inputs (the golden's integrity, the run's lists)
  driverScripts = lib.fileset.toSource {
    root = ../crates/sai-driver/tests/driver-scripts;
    fileset = lib.fileset.fileFilter (f: f.hasExt "ops" && lib.hasPrefix "0" f.name) ../crates/sai-driver/tests/driver-scripts;
  };
  # the one mock libifcs of every Teralynx test (./teralynx-mock.nix)
  mock = callPackage ./teralynx-mock.nix { inherit (saiRs) ifcsHeaders; };
  crate = ../crates/sai-teralynx-parity;
  gold = crate + "/tests/golden";
  lists = ../crates/sai-backend-teralynx/tests;
  rustLib = callPackage ./libsai-teralynx.nix { inherit saiRs; };
  # the Rust test_ifcs (ifcs_e2e, its golden and check map beside it) and the Rust shim of this library's build
  e2e = "${rustLib.e2eTests}/e2e/ifcs_e2e";
  inherit (rustLib) shim;
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
  # the harness's program and the Rust oracle runner it runs (feature cli: not part of the workspace build's outputs)
  tool = saiRs.overrideAttrs (old: {
    pname = "sai-teralynx-parity-tool";
    cargoBuildFlags = [ "-p" "sai-teralynx-parity" "-p" "sai-script" "--features" "sai-teralynx-parity/cli,sai-script/cli" ];
    doCheck = false;
    postInstall = "";
    meta = old.meta // { description = "the Teralynx parity harness's program and the Rust sai-script"; };
  });
  canRun = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  arch = stdenv.hostPlatform.parsed.cpu.name;
  # the fixtures into $fx (CC / CXX: the target's)
  fixturesScript = ''
    MOCK_LIBIFCS=${mock}/mock/libifcs.so ORACLE_SRC=${oracleSrc} PYTHON=${buildPackages.python3}/bin/python3 CC=${stdenv.cc.targetPrefix}cc CXX=${stdenv.cc.targetPrefix}c++ \
      bash ${crate}/build-fixtures.sh $fx ${shim}/lib/libsai-teralynx-shim.so ${saiRs.ifcsHeaders}/include ${nlohmann_json}/include'';
  # the runs over $fx and the gate (toolBin: the harness's command)
  checksScript = toolBin: ''
    lists() {
      echo --areas ${lists}/areas.txt --expected ${lists}/test_ifcs-expected.txt \
        --allow ${crate}/tests/parity-allow.txt --pending ${crate}/tests/parity-pending.txt \
        --driver-scripts ${driverScripts} --oracle-scripts ${oracleSrc}/scripts \
        --test-ifcs ${e2e} --sai-script ${tool}/bin/sai-script
    }
    run() {   # out-name, extra args (a later --rust-lib / --golden wins)
      local name=$1; shift
      ${toolBin} --fixtures $fx --work $NIX_BUILD_TOP/w-$name --golden ${gold} \
        --rust-lib ${rustLib}/lib/libsai.so.1.0.0 $(lists) "$@" > $name.txt 2>&1
    }
    rc=0; run report || rc=$?
    cat report.txt
    cp $NIX_BUILD_TOP/w-report/out/details.txt details.txt
    [ $rc -eq 0 ] || ${if strict then "{ echo 'parity: red (strict build: the strict run decides)'; }" else "{ echo \"parity: RED (exit $rc)\"; exit 1; }"}
    src=0; run strict --strict || src=$?
    sed -n '/^== checks/p; /^== sections/p; /^== oracle/p; /^== verdict/p' strict.txt
    ${lib.optionalString strict ''[ $src -eq 0 ] || { echo "parity (strict): RED (exit $src)"; exit 1; }''}
    # the negative control: a marked Rust call line (create_switch's node create) is red on its section
    nrc=0; run negative --parts sections --mutate ifcs_node_create || nrc=$?
    if [ $nrc -ne 1 ] || ! grep -q 'test_ifcs section main ' negative.txt \
        || ! grep -q 'ifcs_node_create node=0 count=0 attrs=null \[mutated\]' $NIX_BUILD_TOP/w-negative/out/sections/main.rust.txt; then
      echo "negative control: a changed Rust call line was NOT reported (exit $nrc)"; cat negative.txt; exit 1
    fi
    echo "negative control: the changed Rust call line made the run red, as it must"
    brc=0; run negative-bytes --parts checks,sections --rust-lib ${rustLibPoisoned}/lib/libsai.so.1.0.0 || brc=$?
    if [ $brc -ne 1 ] || ! grep -q "^== checks: Rust passes 1047 of 1047" negative-bytes.txt \
        || ! grep -qh "^@B ifcs_[a-z0-9_]*_set({[0-9a-f]*a5a5a5" $NIX_BUILD_TOP/w-negative-bytes/out/sections/*.rust.txt; then
      echo "byte negative control: poisoned key padding was NOT reported on an @B line (exit $brc)"; cat negative-bytes.txt; exit 1
    fi
    echo "byte negative control: the poisoned key padding made the run red on its @B lines (test_ifcs still 1047 / 1047), as it must"
    # the golden negative control: a changed digest in the golden (a test_ifcs section, an oracle step) is red
    cp -r ${gold} neg-golden && chmod -R u+w neg-golden
    flip() {   # file, record tag, field with the name, name: the first sha256 of that record with its first digit changed
      awk -F'\t' -v OFS='\t' -v tag="$2" -v col="$3" -v name="$4" '$1==tag && $col==name && !done { c=substr($3,1,1); $3=(c=="0"?"1":"0") substr($3,2); done=1 } { print }' neg-golden/$1 > neg-golden/$1.new
      cmp -s neg-golden/$1 neg-golden/$1.new && { echo "golden negative control: no record $2 '$4' to change"; exit 1; }
      mv neg-golden/$1.new neg-golden/$1
    }
    flip test_ifcs-cpp.txt section 5 l2
    flip oracle-cpp.txt step 4 '0.3 create L3 table (aclorch)'
    ngrc=0; run negative-golden --strict --golden neg-golden || ngrc=$?
    if [ $ngrc -ne 1 ] || ! grep -q "test_ifcs section l2 " negative-golden.txt \
        || ! grep -q "oracle acl step '0.3 create L3 table (aclorch)'" negative-golden.txt; then
      echo "golden negative control: a changed digest in the golden was NOT reported (exit $ngrc)"; cat negative-golden.txt; exit 1
    fi
    echo "golden negative control: a changed digest in the golden made the run red on its section and step, as it must"
    {
      grep '^== checks' report.txt
      grep '^pending areas' report.txt
      echo "progress mode: exit $rc; strict mode: exit $src ($(grep '^== verdict' strict.txt)); against the golden C++ side (${gold})"
    } > gate.txt
    cat gate.txt'';
  # cross: the fixtures built here, run in ./emulated
  fixtures = stdenv.mkDerivation {
    pname = "sai-teralynx-parity-fixtures-${arch}";
    inherit (saiRs) version;
    dontUnpack = true;
    dontConfigure = true;
    buildPhase = ''
      runHook preBuild
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
    cp report.txt strict.txt negative.txt negative-bytes.txt negative-golden.txt details.txt gate.txt machine $d/
    ${lib.optionalString (reference != null) "cp arch.txt $d/"}
  '';
in
if !canRun then
  assert runnerPkgs != null;
  emulated.overrideAttrs (old: { passthru = { inherit tool rustLib rustLibPoisoned oracleSrc driverScripts mock fixtures; }; })
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
    cp report.txt strict.txt negative.txt negative-bytes.txt negative-golden.txt details.txt gate.txt $d/
    cp -r fixtures/bin fixtures/mock fixtures/cfg $out/libexec/sai-teralynx-parity/
    cp ${tool}/bin/sai-teralynx-parity ${tool}/bin/sai-script $out/bin/
    # an area owner's run from a checkout of rust/sai: that tree's lists and goldens, the store's fixtures and programs
    makeWrapper $out/bin/sai-teralynx-parity $out/bin/sai-teralynx-parity-run \
      --add-flags "--fixtures $out/libexec/sai-teralynx-parity --test-ifcs ${e2e} --sai-script $out/bin/sai-script" \
      --add-flags "--driver-scripts ${driverScripts} --oracle-scripts ${oracleSrc}/scripts" \
      --run 'if [ -f crates/sai-teralynx-parity/Cargo.toml ]; then set -- --golden crates/sai-teralynx-parity/tests/golden --areas crates/sai-backend-teralynx/tests/areas.txt --expected crates/sai-backend-teralynx/tests/test_ifcs-expected.txt --allow crates/sai-teralynx-parity/tests/parity-allow.txt --pending crates/sai-teralynx-parity/tests/parity-pending.txt "$@"; else echo "sai-teralynx-parity-run: run it from rust/sai (the lists are read from the checkout)" >&2; exit 2; fi' \
      --run 'case " $* " in *" --work "*) ;; *) set -- --work "''${SAI_RS_TEST_TMPDIR:-$PWD/target/teralynx-parity}" "$@" ;; esac'
    runHook postInstall
  '';

  dontStrip = true;
  passthru = { inherit tool rustLib rustLibPoisoned oracleSrc driverScripts mock; };

  meta = {
    description = "Parity of the Rust Teralynx libsai with the golden of the C++ one on the mock libifcs (test_ifcs checks, call streams, oracle scripts)";
    license = lib.licenses.asl20;
    platforms = [ "x86_64-linux" ];
  };
}
