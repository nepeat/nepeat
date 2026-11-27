# sai-rs: libsai.so in Rust (virtual backend) + its tests, as a callPackage-able function:
#   pkgs.callPackage ./rust/sai/nix/package.nix { saiSrc = <sonic-sairedis source>; }
# saiSrc: the sonic-sairedis checkout whose SAI submodule syncd is built against
# (ai_re: sonic.sources.sairedis); the bindings are generated from its SAI/inc.
# glueSrc: the directory of the C SDK glue (sdk_glue.h, sdk_glue.c; ai_re/nix/packages/sai-openbcm/src):
# sdk-glue-sys's bindings are generated from and checked against that header.
# cppFrontendSrc: optional, the C++ frontend frozen in ai_re/nix/packages/sai-teralynx/frontend/src:
# the live differentials against it (sai-macsec, sai-macsec-phy's e2e, openbcm-diag) then run;
# without it they skip. The comparisons with deleted C/C++ are goldens in the crates' tests/golden.
# phySeqTables: optional board tables (the AS7772's generated ext-macsec / ext-phy): phy-seq's
# goldens of them then run (PHY_SEQ_TABLES); a table the golden does not know is skipped.
# teralynxSrc: the C++ Teralynx package (ai_re/nix/packages/sai-teralynx), read only: the ctypes file it
# pins, from which ifcs-sys's build generates the IFCS headers with ifcs-gen (IFCS_CTYPES; equal to
# ./ifcs-headers.nix's: IFCS_API_EXPECT) on the targets the
# Teralynx crates are built for (x86_64, aarch64; elsewhere they are empty and need nothing).
# Source: this workspace only (its own lock file), so edits elsewhere, to READMEs or to
# these .nix files do not rebuild it. Tests, clippy and the C ABI harness run on native
# builds; a cross build still does both ABI checks of sai-sys (they need no execution).
{
  lib,
  rustPlatform,
  stdenv,
  clippy,
  rustfmt,
  valgrind,
  saiSrc,
  glueSrc ? ../../../ai_re/nix/packages/sai-openbcm/src,
  cppFrontendSrc ? null,
  phySeqTables ? [ ],
  teralynxSrc ? ../../../ai_re/nix/packages/sai-teralynx,
  callPackage,
}:
let
  ws = ../.;
  glue = builtins.path {
    path = glueSrc;
    name = "sdk-glue-src";
    filter = p: _: builtins.elem (baseNameOf p) [ "sdk_glue.h" "sdk_glue.c" ];
  };
  manifest = lib.importTOML ../Cargo.toml;
  canRun = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  # native checks only (a cross build has no check phase: nothing of valgrind's for the target)
  withValgrind = canRun && lib.meta.availableOn stdenv.buildPlatform valgrind;
  # the Teralynx crates (ifcs-sys, elf-got, ifcs, sai-backend-teralynx): x86_64 (tests on the
  # mock) and aarch64 (the CS8210) only
  teralynxTarget = stdenv.hostPlatform.isx86_64 || stdenv.hostPlatform.isAarch64;
  ifcsHeaders = callPackage ./ifcs-headers.nix { inherit teralynxSrc saiSrc; };
in
rustPlatform.buildRustPackage {
  pname = "sai-rs";
  inherit (manifest.workspace.package) version;
  src = lib.fileset.toSource {
    root = ws;
    fileset = lib.fileset.unions [
      (ws + "/Cargo.toml")
      (ws + "/Cargo.lock")
      (ws + "/clippy.toml")
      (ws + "/rustfmt.toml")
      (lib.fileset.fileFilter (f: f.hasExt "rs" || f.hasExt "c" || f.hasExt "cpp" || f.hasExt "h" || f.hasExt "mdio" || f.hasExt "txt" || f.hasExt "ops" || f.hasExt "sh" || f.name == "Cargo.toml") (ws + "/crates"))
      # ifcs-gen's templates (.h.in) and its synthetic test fixtures (a ctypesgen-shaped .py)
      (ws + "/crates/ifcs-gen/templates")
      (ws + "/crates/ifcs-gen/tests/fixtures")
      # ifcs-gen's goldens (header digests, model-fact digest: .sha256 files, H5-0)
      (ws + "/crates/ifcs-gen/tests/golden")
    ];
  };
  cargoLock.lockFile = ws + "/Cargo.lock";

  nativeBuildInputs = [ rustPlatform.bindgenHook ];
  nativeCheckInputs = [ clippy rustfmt ] ++ lib.optional withValgrind valgrind;

  env = {
    SAI_INC = "${saiSrc}/SAI/inc";
    # the C compiler must confirm the bindings (sai-sys/build.rs); a build without it fails
    SAI_SYS_ABI_CHECK = "require";
    SAI_SYS_ABI_CC = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";   # the compiler syncd is built with
    # sdk-glue-sys: the glue's header
    SDK_GLUE_INC = "${glue}";
  } // lib.optionalAttrs teralynxTarget {
    # ifcs-sys: the generated IFCS headers, checked by the target's C compiler (also the one the
    # Teralynx crates build their C test fixtures with)
    IFCS_CTYPES = "${ifcsHeaders.ifcsCtypes}";
    IFCS_API_EXPECT = "${ifcsHeaders}/include";
    IFCS_SYS_ABI_CHECK = "require";
    IFCS_SYS_ABI_CC = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
  } // lib.optionalAttrs (cppFrontendSrc != null) { SAI_OPENBCM_SRC = "${cppFrontendSrc}"; }
    // lib.optionalAttrs (phySeqTables != [ ]) { PHY_SEQ_TABLES = lib.concatStringsSep ":" (map toString phySeqTables); };

  # bindgenHook hands libclang nixpkgs' name of the big-endian ppc64 ELFv2 platform as the
  # target; clang does not know that spelling (it then assumes ELFv1 and looks for glibc
  # headers an ELFv2 sysroot does not have). The same triple the way clang spells it:
  preBuild = lib.optionalString (lib.hasSuffix "gnuabielfv2" stdenv.hostPlatform.config) ''
    export BINDGEN_EXTRA_CLANG_ARGS="${"$"}{BINDGEN_EXTRA_CLANG_ARGS/--target=${stdenv.hostPlatform.config}/--target=${lib.removeSuffix "abielfv2" stdenv.hostPlatform.config} -mabi=elfv2}"
  '';

  # The installed libsai.so must not carry the test port: sai-test-port turns on sai-core's
  # `testing` feature, and one cargo invocation unifies features across the members it builds,
  # so it stays out of the build (its tests still run: cargoTestFlags). cargoInstallHook copies
  # what this build phase produced (${releaseDir}-tmp), not what the check phase rebuilds.
  cargoBuildFlags = [ "--workspace" "--exclude" "sai-test-port" ];
  cargoTestFlags = [ "--workspace" ];
  doCheck = canRun;
  preCheck = ''
    cargo fmt --all --check
    cargo clippy --offline --workspace --all-targets --profile "$cargoCheckType" -- -D warnings
  '';
  # sdk-glue's real FFI path (feature real: real.rs, all 75 wrappers, the callback trampolines from
  # C threads) against the C stub of sdk_glue.h instead of sdk_glue.c + libsdk
  # (crates/sdk-glue/tests/real-stub.sh): its own cargo invocation and target dir, never in the
  # workspace run (a workspace build with real would need SDK_GLUE_LINK for everything); then again
  # under valgrind's memcheck (any error or definite / indirect leak fails).
  postCheck = lib.optionalString canRun ''
    CARGO_TARGET_DIR=$PWD/target-real-stub bash crates/sdk-glue/tests/real-stub.sh
  '' + lib.optionalString withValgrind ''
    CARGO_TARGET_DIR=$PWD/target-real-stub VALGRIND=${valgrind}/bin/valgrind bash crates/sdk-glue/tests/real-stub.sh --valgrind
  '';

  # cargoInstallHook installs binaries and libraries of every workspace member; keep the
  # cdylib as syncd expects to find it.
  postInstall = ''
    if [ -e $out/lib/libsai.so ] && [ ! -L $out/lib/libsai.so ]; then
      mv $out/lib/libsai.so $out/lib/libsai.so.1.0.0
      ln -s libsai.so.1.0.0 $out/lib/libsai.so.1
      ln -s libsai.so.1 $out/lib/libsai.so
    fi
  '';

  passthru = {
    saiHeaders = "${saiSrc}/SAI/inc";
    inherit ifcsHeaders teralynxSrc;
  };

  meta = {
    description = "SAI 1.19 libsai.so in Rust: safe core, virtual backend, small unsafe C-ABI rim (first slice)";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
