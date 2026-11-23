# openbcm-mdio-script in Rust (crates/mdio-tool): the boot-time MDIO script runner.
#   pkgs.callPackage ./mdio-tool.nix { }                         diag-socket backend only (any platform):
#                                                                --check and --verify-only --diag-socket
#   ppc.callPackage ./mdio-tool.nix { libsdk = <openbcm libsdk>; glueSrc = <sai-openbcm/src>; }
#                                                                + the SDK backend: sdk_glue.c compiled with
#                                                                the SDK's cflags, libsdk linked (ppc64 BE)
# glueSrc: the C SDK glue's directory (sdk_glue.c / sdk_glue.h; ai_re/nix/packages/sai-openbcm/src):
# with libsdk it is compiled in; natively the tests run a second time on the glue's FFI module over
# the MDIO stub (crates/mdio-tool/tests/c/mdio_stub.c). The tool's comparison with the deleted C
# tool is a golden (crates/mdio-tool/tests/golden).
# Output: bin/openbcm-mdio-script-rs. passthru.asOpenbcmMdioScript: the same under the C tool's
# name, bin/openbcm-mdio-script (what the NixOS module and the units VM test call). The SAI
# package (sai-openbcm-rs.nix) carries the same tool; this one builds it alone.
# With libsdk the binary links the Broadcom SDK: not freely redistributable.
{
  lib,
  rustPlatform,
  stdenv,
  runCommand,
  libsdk ? null,
  glueSrc ? null,
}:
let
  ws = ../.;
  manifest = lib.importTOML ../Cargo.toml;
  withSdk = libsdk != null;
  native = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  crates = [ "mdio-tool" "mdio-report" "mdio-script" "sdk-glue" ];
  self = rustPlatform.buildRustPackage {
    pname = if withSdk then "openbcm-mdio-script-rs" else "openbcm-mdio-script-rs-nosdk";
    inherit (manifest.workspace.package) version;
    # The whole workspace's manifests (cargo reads every member's), the sources of all crates
    # (tests' goldens and scripts included: include_str!).
    src = lib.fileset.toSource {
      root = ws;
      fileset = lib.fileset.unions [
        (ws + "/Cargo.toml")
        (ws + "/Cargo.lock")
        (ws + "/clippy.toml")
        (ws + "/rustfmt.toml")
        (lib.fileset.fileFilter (f: f.hasExt "rs" || f.hasExt "c" || f.hasExt "cpp" || f.hasExt "h" || f.hasExt "mdio" || f.hasExt "txt" || f.hasExt "ops" || f.name == "Cargo.toml") (ws + "/crates"))
      ];
    };
    cargoLock.lockFile = ws + "/Cargo.lock";
    # sdk-glue-sys's build script runs bindgen (libclang)
    nativeBuildInputs = [ rustPlatform.bindgenHook ];

    env = lib.optionalAttrs (glueSrc != null) {
      # the declarations of sdk-glue-sys must agree with this header, or the build fails
      SDK_GLUE_INC = "${glueSrc}";
      SDK_GLUE_SYS_CHECK = "require";
      SDK_GLUE_SYS_CC = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
    } // lib.optionalAttrs withSdk {
      SDK_GLUE_RPATH = "${libsdk}/lib";
    };

    # bindgenHook hands libclang nixpkgs' spelling of the big-endian ppc64 ELFv2 target, which clang
    # refuses; the triple the way clang spells it (as package.nix does). Then the SDK glue: C,
    # compiled with exactly the SDK's own defines (as the C++ package's Makefile did), handed to
    # sdk-glue-sys as an archive together with libsdk.
    preBuild = lib.optionalString (lib.hasSuffix "gnuabielfv2" stdenv.hostPlatform.config) ''
      export BINDGEN_EXTRA_CLANG_ARGS="${"$"}{BINDGEN_EXTRA_CLANG_ARGS/--target=${stdenv.hostPlatform.config}/--target=${lib.removeSuffix "abielfv2" stdenv.hostPlatform.config} -mabi=elfv2}"
    '' + lib.optionalString withSdk ''
      mkdir -p $NIX_BUILD_TOP/sdk-glue
      ${stdenv.cc.targetPrefix}cc -std=gnu11 -fPIC -Wall -Wno-unused-function -O2 -g \
        $(cat ${libsdk}/share/openbcm/cflags) \
        -I${libsdk}/include/openbcm/include -I${libsdk}/include/openbcm/bde \
        -I${libsdk}/include/openbcm/kmod -I${libsdk}/include/openbcm/phymod \
        -I${glueSrc} -c ${glueSrc}/sdk_glue.c -o $NIX_BUILD_TOP/sdk-glue/sdk_glue.o
      ${stdenv.cc.targetPrefix}ar rcs $NIX_BUILD_TOP/sdk-glue/libob_sdk_glue.a $NIX_BUILD_TOP/sdk-glue/sdk_glue.o
      export SDK_GLUE_LINK="$NIX_BUILD_TOP/sdk-glue/libob_sdk_glue.a -L${libsdk}/lib -lsdk -lpthread"
    '';

    cargoBuildFlags = [ "-p" "mdio-tool" ] ++ lib.optionals withSdk [ "--features" "sdk-real" ];
    # Tests run on sdk-glue's Rust fake of the glue (dev-dependencies), which the plain bring-ups
    # use only where the real glue is not linked: native, SDK-less builds only.
    doCheck = native && !withSdk;
    cargoTestFlags = lib.concatMap (c: [ "-p" c ]) crates;
    # Then mdio-tool's tests once more with the real-glue module (sdk-glue's FFI calls, feature
    # sdk-real), linked with the C stub of the glue's MDIO side (crates/mdio-tool/tests/c/
    # mdio_stub.c) instead of sdk_glue.c +
    # libsdk: what executes sdk-glue's init_mdio / MDIO / temperature / errmsg FFI off the switch.
    postCheck = lib.optionalString (glueSrc != null) ''
      mkdir -p $NIX_BUILD_TOP/stub-glue
      ${stdenv.cc.targetPrefix}cc -std=gnu11 -O1 -g -fPIC -Wall -Wextra -Werror -I${glueSrc} \
        -c crates/mdio-tool/tests/c/mdio_stub.c -o $NIX_BUILD_TOP/stub-glue/mdio_stub.o
      ${stdenv.cc.targetPrefix}ar rcs $NIX_BUILD_TOP/stub-glue/libob_sdk_glue.a $NIX_BUILD_TOP/stub-glue/mdio_stub.o
      SDK_GLUE_LINK=$NIX_BUILD_TOP/stub-glue/libob_sdk_glue.a \
        cargo test --release -j $NIX_BUILD_CORES -p mdio-tool --features sdk-real
    '';

    passthru.asOpenbcmMdioScript = runCommand "openbcm-mdio-script-from-rust" { } ''
      mkdir -p $out/bin
      ln -s ${self}/bin/openbcm-mdio-script-rs $out/bin/openbcm-mdio-script
    '';

    meta = {
      description = "openbcm-mdio-script in Rust: MDIO scripts for PHYs without an SDK driver, at boot";
      license = if withSdk then [ lib.licenses.asl20 lib.licenses.unfreeRedistributable ] else lib.licenses.asl20;
      platforms = lib.platforms.linux;
      mainProgram = "openbcm-mdio-script-rs";
    };
  };
in
assert withSdk -> glueSrc != null;
self
