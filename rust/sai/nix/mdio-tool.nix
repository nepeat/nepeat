# openbcm-mdio-script in Rust (crates/mdio-tool): the boot-time MDIO script runner.
#   pkgs.callPackage ./mdio-tool.nix { }                         diag-socket backend only (any platform):
#                                                                --check and --verify-only --diag-socket
#   ppc.callPackage ./mdio-tool.nix { libsdk = <openbcm libsdk>; saiOpenbcmSrc = <sai-openbcm/src>; }
#                                                                + the SDK backend: sdk_glue.c compiled with
#                                                                the SDK's cflags, libsdk linked (ppc64 BE)
# saiOpenbcmSrc: sai-openbcm's src directory. With libsdk it provides sdk_glue.c / sdk_glue.h (the
# glue stays C). On a native build it also lets the tests compare the tool with the C tool built
# from the same directory (crates/mdio-tool/tests/differential.rs); without it they skip.
# Output: bin/openbcm-mdio-script-rs. passthru.asOpenbcmMdioScript: the same under the C tool's
# name, bin/openbcm-mdio-script (what the NixOS module and the units VM test call).
# With libsdk the binary links the Broadcom SDK: not freely redistributable.
{
  lib,
  rustPlatform,
  stdenv,
  runCommand,
  libsdk ? null,
  saiOpenbcmSrc ? null,
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
    # The whole workspace's manifests (cargo reads every member's), the sources of all crates.
    src = lib.fileset.toSource {
      root = ws;
      fileset = lib.fileset.unions [
        (ws + "/Cargo.toml")
        (ws + "/Cargo.lock")
        (ws + "/clippy.toml")
        (ws + "/rustfmt.toml")
        (lib.fileset.fileFilter (f: f.hasExt "rs" || f.hasExt "c" || f.hasExt "cpp" || f.hasExt "h" || f.hasExt "mdio" || f.name == "Cargo.toml") (ws + "/crates"))
      ];
    };
    cargoLock.lockFile = ws + "/Cargo.lock";
    # sdk-glue-sys's build script runs bindgen (libclang)
    nativeBuildInputs = [ rustPlatform.bindgenHook ];

    env = lib.optionalAttrs (saiOpenbcmSrc != null) {
      # the declarations of sdk-glue-sys must agree with this header, or the build fails
      SDK_GLUE_INC = "${saiOpenbcmSrc}";
      SDK_GLUE_SYS_CHECK = "require";
      SDK_GLUE_SYS_CC = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
    } // lib.optionalAttrs (saiOpenbcmSrc != null && native) {
      # tests: build the C tool from the same sources and compare (no skipping)
      MDIO_SCRIPT_C_DIR = "${saiOpenbcmSrc}";
      MDIO_TOOL_C_PARITY = "require";
    } // lib.optionalAttrs withSdk {
      SDK_GLUE_RPATH = "${libsdk}/lib";
    };

    # The SDK glue is C, compiled with exactly the SDK's own defines (as sai-openbcm's Makefile
    # does), then handed to sdk-glue-sys as an archive together with libsdk.
    preBuild = lib.optionalString withSdk ''
      mkdir -p $NIX_BUILD_TOP/sdk-glue
      ${stdenv.cc.targetPrefix}cc -std=gnu11 -fPIC -Wall -Wno-unused-function -O2 -g \
        $(cat ${libsdk}/share/openbcm/cflags) \
        -I${libsdk}/include/openbcm/include -I${libsdk}/include/openbcm/bde \
        -I${libsdk}/include/openbcm/kmod -I${libsdk}/include/openbcm/phymod \
        -I${saiOpenbcmSrc} -c ${saiOpenbcmSrc}/sdk_glue.c -o $NIX_BUILD_TOP/sdk-glue/sdk_glue.o
      ${stdenv.cc.targetPrefix}ar rcs $NIX_BUILD_TOP/sdk-glue/libob_sdk_glue.a $NIX_BUILD_TOP/sdk-glue/sdk_glue.o
      export SDK_GLUE_LINK="$NIX_BUILD_TOP/sdk-glue/libob_sdk_glue.a -L${libsdk}/lib -lsdk -lpthread"
    '';

    cargoBuildFlags = [ "-p" "mdio-tool" ] ++ lib.optionals withSdk [ "--features" "sdk-real" ];
    # Tests link the fake glue (dev-dependencies), which excludes the real one: native,
    # SDK-less builds only. The SDK-linked build is tested as that same code on the fake.
    doCheck = native && !withSdk;
    cargoTestFlags = lib.concatMap (c: [ "-p" c ]) crates;

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
assert withSdk -> saiOpenbcmSrc != null;
self
