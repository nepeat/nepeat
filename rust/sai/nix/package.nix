# sai-rs: libsai.so in Rust (virtual backend) + its tests, as a callPackage-able function:
#   pkgs.callPackage ./rust/sai/nix/package.nix { saiSrc = <sonic-sairedis source>; }
# saiSrc: the sonic-sairedis checkout whose SAI submodule syncd is built against
# (ai_re: sonic.sources.sairedis); the bindings are generated from its SAI/inc.
# mdioScriptC: optional directory with mdio_script.{c,h} (ai_re/nix/packages/sai-openbcm/src):
# the mdio-script crate's differential test then runs against the C interpreter.
# Source: this workspace only (its own lock file), so edits elsewhere, to READMEs or to
# these .nix files do not rebuild it. Tests, clippy and the C ABI harness run on native
# builds; a cross build still does both ABI checks of sai-sys (they need no execution).
{
  lib,
  rustPlatform,
  stdenv,
  clippy,
  rustfmt,
  saiSrc,
  mdioScriptC ? null,
}:
let
  ws = ../.;
  manifest = lib.importTOML ../Cargo.toml;
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
      (lib.fileset.fileFilter (f: f.hasExt "rs" || f.hasExt "c" || f.hasExt "mdio" || f.name == "Cargo.toml") (ws + "/crates"))
    ];
  };
  cargoLock.lockFile = ws + "/Cargo.lock";

  nativeBuildInputs = [ rustPlatform.bindgenHook ];
  nativeCheckInputs = [ clippy rustfmt ];

  env = {
    SAI_INC = "${saiSrc}/SAI/inc";
    # the C compiler must confirm the bindings (sai-sys/build.rs); a build without it fails
    SAI_SYS_ABI_CHECK = "require";
    SAI_SYS_ABI_CC = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";   # the compiler syncd is built with
  } // lib.optionalAttrs (mdioScriptC != null) { MDIO_SCRIPT_C_DIR = "${mdioScriptC}"; };

  # bindgenHook hands libclang nixpkgs' name of the big-endian ppc64 ELFv2 platform as the
  # target; clang does not know that spelling (it then assumes ELFv1 and looks for glibc
  # headers an ELFv2 sysroot does not have). The same triple the way clang spells it:
  preBuild = lib.optionalString (lib.hasSuffix "gnuabielfv2" stdenv.hostPlatform.config) ''
    export BINDGEN_EXTRA_CLANG_ARGS="${"$"}{BINDGEN_EXTRA_CLANG_ARGS/--target=${stdenv.hostPlatform.config}/--target=${lib.removeSuffix "abielfv2" stdenv.hostPlatform.config} -mabi=elfv2}"
  '';

  cargoBuildFlags = [ "--workspace" ];
  cargoTestFlags = [ "--workspace" ];
  doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  preCheck = ''
    cargo fmt --all --check
    cargo clippy --offline --workspace --all-targets --profile "$cargoCheckType" -- -D warnings
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

  passthru.saiHeaders = "${saiSrc}/SAI/inc";

  meta = {
    description = "SAI 1.19 libsai.so in Rust: safe core, virtual backend, small unsafe C-ABI rim (first slice)";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
