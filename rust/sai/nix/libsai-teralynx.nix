# libsai.so of the Rust SAI with the Teralynx 7 backend (crate sai-backend-teralynx, the CS8210-32X:
# libifcs.so dlopen'ed at run time, nothing vendor linked). README "Teralynx backend".
#   native (x86_64: the checks below run):  pkgs.callPackage ./libsai-teralynx.nix { saiRs = <./package.nix>; cppTeralynx = <sai-teralynx>; }
#   aarch64 (the CS8210):                   pkgsCross.aarch64-multiplatform.callPackage ./libsai-teralynx.nix { saiRs = <./package.nix for it>; }
# saiRs: ./package.nix for the same pkgs (its passthru: the SAI headers, the generated IFCS headers,
# the C++ Teralynx package's source). cppTeralynx: optional, the C++ sai-teralynx package for the same
# pkgs (its lib/libsai.so.1.0.0: the reference of the create / remove call-log comparison).
#
# Checked at build time (results in $out/share/sai-rs/):
#   - -z defs, soname libsai.so.1, the exports of the virtual-backend library of saiRs (libsai-teralynx.txt)
#   - natively: the Teralynx crates' tests (ifcs-sys, elf-got, ifcs, sai-backend-teralynx), then
#     crates/sai-backend-teralynx/tests/test-ifcs.sh: test_ifcs (unmodified) on this library with the C
#     mock libifcs and the C shim, every FAIL line one of tests/test_ifcs-expected.txt; the IFCS call log
#     of create_switch / remove_switch equal to the C++ library's (test_ifcs.txt, create_remove.diff);
#     then the same test in Rust (crates/sai-backend-teralynx/tests/ifcs_e2e: the built libsai through
#     sai-client on the same mock and shim; its checks, in order, equal to the C++ test's golden;
#     ifcs_e2e.txt)
#   - a cross build: the bindings' layout checks for the target (ifcs-sys's build, the target's cc);
#     passthru.testBundle has this library, the mock, the shim, test_ifcs and the call-log driver built
#     for the target, to run under binfmt (./teralynx-target-run.nix), and the ifcs crate's ABI test
#     (crates/ifcs/tests/abi.rs with its C fixture: the byte twins passed as the target's C passes the
#     structs, every byte libifcs receives defined)
{
  lib,
  stdenv,
  callPackage,
  saiRs,
  cppTeralynx ? null,
}:
let
  cc = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
  cxx = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}c++";
  bin = "${stdenv.cc.bintools.bintools}/bin/${stdenv.cc.targetPrefix}";
  triple = stdenv.hostPlatform.rust.rustcTarget;
  canRun = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  tl = saiRs.teralynxSrc;
  tlFiles = builtins.path {
    path = tl;
    name = "sai-teralynx-test-src";
    # what the test reads: the shim (C), test_ifcs; nothing of the C++ library itself (the mock: ./teralynx-mock.nix)
    filter = p: t:
      let rel = lib.removePrefix (toString tl + "/") (toString p); in
      rel == "src" || rel == "tests"
      || builtins.elem rel [ "src/shim.c" "src/ifcs_guard.c" "tests/harness.h" ]
      || (lib.hasPrefix "tests/" rel && lib.hasSuffix ".cpp" rel);
  };
  gen = "${saiRs.ifcsHeaders}/include";
  # the one mock libifcs of every Teralynx test (./teralynx-mock.nix)
  mock = callPackage ./teralynx-mock.nix { inherit (saiRs) ifcsHeaders; };
  libsai = saiRs.overrideAttrs (old: {
  pname = "sai-rs-libsai-teralynx";

  # Only the library: the other workspace members are package.nix's.
  cargoBuildFlags = [ "-p" "sai-ffi" "--features" "teralynx" ];

  env = (old.env or { }) // {
    # every link for the target, i.e. libsai.so: an unresolved symbol fails the build
    NIX_LDFLAGS = "-z defs";
  };

  doCheck = canRun;
  cargoTestFlags = [ "-p" "ifcs-sys" "-p" "elf-got" "-p" "ifcs" "-p" "sai-backend-teralynx" ];
  preCheck = ""; # fmt and clippy: package.nix's check
  postCheck = lib.optionalString canRun ''
    so=$PWD/target/${triple}/$cargoBuildType/libsai.so
    test -f "$so"
    mkdir -p $NIX_BUILD_TOP/tl
    CC=${cc} CXX=${cxx} MOCK_LIBIFCS=${mock}/mock/libifcs.so bash crates/sai-backend-teralynx/tests/test-ifcs.sh $NIX_BUILD_TOP/tl "$so" ${tlFiles} ${gen} ${saiRs.saiHeaders} \
      ${lib.optionalString (cppTeralynx != null) "${cppTeralynx}/lib/libsai.so.1.0.0"}
    # test_ifcs in Rust: the cargo test binary of the check phase, now with the libsai, the mock and the
    # shim that test-ifcs.sh put in $NIX_BUILD_TOP/tl/lib (a short scratch path: the diag socket)
    e2e=$(ls -t target/${triple}/$cargoBuildType/deps/ifcs_e2e-* | grep -v '\.d$' | head -1)
    test -x "$e2e"
    IFCS_E2E_REQUIRE=1 IFCS_E2E_LIBSAI=$NIX_BUILD_TOP/tl/lib/libsai.so.1.0.0 IFCS_E2E_MOCK=${mock}/mock/libifcs.so \
      IFCS_E2E_DIR=$NIX_BUILD_TOP/tl/e2e "$e2e" --nocapture > $NIX_BUILD_TOP/tl/ifcs_e2e.txt 2>&1 || { cat $NIX_BUILD_TOP/tl/ifcs_e2e.txt; exit 1; }
    tail -n 5 $NIX_BUILD_TOP/tl/ifcs_e2e.txt
  '';

  postInstall = (old.postInstall or "") + ''
    mkdir -p $out/share/sai-rs
    so=$out/lib/libsai.so.1.0.0
    report=$out/share/sai-rs/libsai-teralynx.txt
    exports() { ${bin}nm -D --defined-only "$1" | awk '{ print $3 }' | LC_ALL=C sort; }
    {
      echo "libsai.so with the teralynx backend (libifcs dlopen'ed at run time), ${stdenv.hostPlatform.config}"
      echo "size: $(stat -c %s $so) bytes"
      echo "== dynamic section"
      ${bin}readelf -d $so | grep -E 'SONAME|NEEDED|RUNPATH|RPATH'
      echo "== exported symbols"
      exports $so
      echo "== undefined symbols: $(${bin}nm -D --undefined-only $so | wc -l), by library version"
      ${bin}nm -D --undefined-only $so | sed -n 's/.*@\(.*\)/\1/p' | LC_ALL=C sort | uniq -c
    } > $report
    cat $report
    ${bin}readelf -d $so | grep -q 'SONAME.*\[libsai.so.1\]' || { echo "wrong soname"; exit 1; }
    exports $so > exports.teralynx
    exports ${saiRs}/lib/libsai.so > exports.virtual
    diff -u exports.virtual exports.teralynx || { echo "exported symbols differ from the virtual-backend library"; exit 1; }
    if ${bin}nm -D --undefined-only $so | grep -q ' ifcs_'; then echo "ifcs_* symbols are linked, not dlopen'ed"; exit 1; fi
    ${lib.optionalString canRun ''
      cp $NIX_BUILD_TOP/tl/test_ifcs-result.txt $out/share/sai-rs/test_ifcs.txt
      cp $NIX_BUILD_TOP/tl/create_remove-rust.txt $out/share/sai-rs/create_remove.txt
      cp $NIX_BUILD_TOP/tl/ifcs_e2e.txt $out/share/sai-rs/ifcs_e2e.txt
      if [ -f $NIX_BUILD_TOP/tl/create_remove.diff ]; then cp $NIX_BUILD_TOP/tl/create_remove.diff $out/share/sai-rs/; fi
    ''}
  '';

  passthru = (old.passthru or { }) // { inherit cppTeralynx tlFiles ifcsTests e2eTests; testBundle = bundle; };

  meta = old.meta // {
    description = "Rust libsai.so with the Teralynx 7 backend (libifcs loaded at run time) for the CS8210-32X";
  };
});

  # the ifcs crate's ABI test program and its C fixture built for the target (tests/abi.rs: the byte
  # twins of by-value structs on the target's calling convention, the keys' and attributes' padding),
  # for the bundle (./teralynx-target-run.nix runs it with IFCS_ABI_FIXTURE)
  ifcsTests = saiRs.overrideAttrs (old: {
    pname = "sai-rs-ifcs-abi-test";
    cargoBuildFlags = [ "-p" "ifcs" "--tests" ];
    doCheck = false;
    installPhase = ''
      runHook preInstall
      mkdir -p $out/ifcs
      exe=$(ls -t target/${triple}/$cargoBuildType/deps/abi-* | grep -v '\.d$' | head -1)
      install -m755 "$exe" $out/ifcs/abi
      install -m755 target/${triple}/$cargoBuildType/build/ifcs-*/out/target-tests/libifcs_abi_fixture.so $out/ifcs/
      runHook postInstall
    '';
    postInstall = "";
    meta = old.meta // { description = "the ifcs crate's ABI test (tests/abi.rs) and its C fixture, built for the target"; };
  });

  # the Rust end-to-end test (crates/sai-backend-teralynx/tests/ifcs_e2e) built for the target, for the bundle
  # (./teralynx-target-run.nix runs it on the bundle's libsai, mock and shim with its golden and check map)
  e2eTests = saiRs.overrideAttrs (old: {
    pname = "sai-rs-ifcs-e2e-test";
    cargoBuildFlags = [ "-p" "sai-backend-teralynx" "--tests" ];
    doCheck = false;
    installPhase = ''
      runHook preInstall
      mkdir -p $out/e2e
      exe=$(ls -t target/${triple}/$cargoBuildType/deps/ifcs_e2e-* | grep -v '\.d$' | head -1)
      install -m755 "$exe" $out/e2e/ifcs_e2e
      cp -r crates/sai-backend-teralynx/tests/ifcs_e2e/golden crates/sai-backend-teralynx/tests/ifcs_e2e/CHECK-MAP.txt $out/e2e/
      runHook postInstall
    '';
    postInstall = "";
    meta = old.meta // { description = "the Teralynx end-to-end test (tests/ifcs_e2e) built for the target, with its golden"; };
  });

  # this library and the C test programs built for the target, to run where the target executes
  # (./teralynx-target-run.nix: binfmt); the same programs test-ifcs.sh builds natively
  bundle = stdenv.mkDerivation {
    pname = "sai-rs-teralynx-test-bundle";
    inherit (libsai) version;
    dontUnpack = true;
    dontFixup = true;
    buildPhase = ''
      mkdir -p $out/lib $out/mock
      cp ${libsai}/lib/libsai.so.1.0.0 $out/lib/libsai.so.1.0.0
      ln -s libsai.so.1.0.0 $out/lib/libsai.so.1
      ln -s libsai.so.1 $out/lib/libsai.so
      warn="-Wall -Wextra -Wno-unused-parameter -Wno-missing-field-initializers"
      ${cc} -std=gnu11 -fPIC -shared -Wall -fno-builtin -fno-tree-loop-distribute-patterns -O2 -g \
        -Wl,-soname,libsai-teralynx-shim.so -o $out/lib/libsai-teralynx-shim.so ${tlFiles}/src/shim.c ${tlFiles}/src/ifcs_guard.c -lpthread
      cp ${mock}/mock/libifcs.so $out/mock/libifcs.so
      ${cxx} -std=c++17 $warn -O2 -g -I${saiRs.saiHeaders} -I${saiRs.saiHeaders}/../experimental -I${tlFiles}/tests -o $out/lib/test_ifcs \
        ${tlFiles}/tests/test_ifcs.cpp ${tlFiles}/tests/t_*.cpp $out/lib/libsai.so.1.0.0 -Wl,-rpath,'$ORIGIN' -ldl -lpthread
      ${cc} -std=gnu11 -Wall -Wextra -O1 -g -I${saiRs.saiHeaders} -I${saiRs.saiHeaders}/../experimental \
        -o $out/create_remove ${../crates/sai-backend-teralynx/tests/c/create_remove.c} -ldl
      cp ${../crates/sai-backend-teralynx/tests/test-ifcs.sh} $out/test-ifcs.sh
      cp ${../crates/sai-backend-teralynx/tests/test_ifcs-expected.txt} $out/test_ifcs-expected.txt
      cp ${../crates/sai-backend-teralynx/tests/areas.txt} $out/areas.txt
      cp ${../crates/sai-backend-teralynx/tests/post-init-regs.txt} $out/post-init-regs.txt
      cp ${../crates/sai-backend-teralynx/tests/diag-regs.txt} $out/diag-regs.txt
      cp -r ${ifcsTests}/ifcs $out/ifcs
      cp -r ${e2eTests}/e2e $out/e2e
    '';
    installPhase = "true";
  };
in
libsai
