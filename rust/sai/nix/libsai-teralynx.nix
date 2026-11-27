# libsai.so of the Rust SAI with the Teralynx 7 backend (crate sai-backend-teralynx, the CS8210-32X:
# libifcs.so dlopen'ed at run time, nothing vendor linked). README "Teralynx backend".
#   native (x86_64: the checks below run):  pkgs.callPackage ./libsai-teralynx.nix { saiRs = <./package.nix>; }
#   aarch64 (the CS8210):                   pkgsCross.aarch64-multiplatform.callPackage ./libsai-teralynx.nix { saiRs = <./package.nix for it>; }
# saiRs: ./package.nix for the same pkgs (its passthru: the SAI headers, the generated IFCS headers).
#
# Checked at build time (results in $out/share/sai-rs/):
#   - -z defs, soname libsai.so.1, the exports of the virtual-backend library of saiRs (libsai-teralynx.txt)
#   - natively: the Teralynx crates' tests (ifcs-sys, elf-got, ifcs, sai-backend-teralynx), then
#     crates/sai-backend-teralynx/tests/test-ifcs.sh: this library with the C mock libifcs and the Rust shim
#     (./ifcs-shim.nix): the IFCS call log of create_switch / remove_switch (create_remove.txt) and the
#     post-init register list (test_ifcs.txt); then test_ifcs in Rust (crates/sai-backend-teralynx/tests/ifcs_e2e:
#     the built libsai through sai-client on the same mock and shim; its checks, in order, equal to the C++
#     test's golden; ifcs_e2e.txt). The C++ test_ifcs and the C++ library's call log are gone (H5, 2026-11-27):
#     the parity harness (./teralynx-parity.nix) holds the IFCS calls of every test_ifcs section to the C++ golden.
#   - a cross build: the bindings' layout checks for the target (ifcs-sys's build, the target's cc);
#     passthru.testBundle has this library, the mock, the Rust shim, the call-log driver and the Rust test_ifcs
#     built for the target, to run under binfmt (./teralynx-target-run.nix), and the ifcs crate's ABI test
#     (crates/ifcs/tests/abi.rs with its C fixture: the byte twins passed as the target's C passes the
#     structs, every byte libifcs receives defined)
{
  lib,
  stdenv,
  callPackage,
  saiRs,
}:
let
  cc = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
  bin = "${stdenv.cc.bintools.bintools}/bin/${stdenv.cc.targetPrefix}";
  triple = stdenv.hostPlatform.rust.rustcTarget;
  canRun = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  gen = "${saiRs.ifcsHeaders}/include";
  # the one mock libifcs of every Teralynx test (./teralynx-mock.nix)
  mock = callPackage ./teralynx-mock.nix { inherit (saiRs) ifcsHeaders; };
  # the Rust libsai-teralynx-shim.so for the same platform (./ifcs-shim.nix: its checks), next to the libsai
  shim = (callPackage ./ifcs-shim.nix { inherit saiRs; }).shim;
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
    CC=${cc} MOCK_LIBIFCS=${mock}/mock/libifcs.so bash crates/sai-backend-teralynx/tests/test-ifcs.sh $NIX_BUILD_TOP/tl "$so" \
      ${shim}/lib/libsai-teralynx-shim.so ${gen} ${saiRs.saiHeaders}
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
    ''}
  '';

  passthru = (old.passthru or { }) // { inherit shim ifcsTests e2eTests; testBundle = bundle; };

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

  # this library, the Rust shim, the mock, the call-log driver and the Rust test_ifcs built for the target, to run
  # where the target executes (./teralynx-target-run.nix: binfmt); what test-ifcs.sh and the check phase run natively
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
      cp ${shim}/lib/libsai-teralynx-shim.so $out/lib/libsai-teralynx-shim.so
      cp ${mock}/mock/libifcs.so $out/mock/libifcs.so
      ${cc} -std=gnu11 -Wall -Wextra -O1 -g -I${saiRs.saiHeaders} -I${saiRs.saiHeaders}/../experimental \
        -o $out/create_remove ${../crates/sai-backend-teralynx/tests/c/create_remove.c} -ldl
      cp ${../crates/sai-backend-teralynx/tests/test-ifcs.sh} $out/test-ifcs.sh
      cp ${../crates/sai-backend-teralynx/tests/c/create_remove-cpp.txt} $out/create_remove-cpp.txt
      cp ${../crates/sai-backend-teralynx/tests/post-init-regs.txt} $out/post-init-regs.txt
      cp ${../crates/sai-backend-teralynx/tests/diag-regs.txt} $out/diag-regs.txt
      cp -r ${ifcsTests}/ifcs $out/ifcs
      cp -r ${e2eTests}/e2e $out/e2e
    '';
    installPhase = "true";
  };
in
libsai
