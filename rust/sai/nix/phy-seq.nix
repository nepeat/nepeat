# phy-seq as a C library for the C++ libsai (ai_re/nix/packages/sai-openbcm, argument rustPhySeq):
#   $out/lib/libphy_seq_capi.a   the static library (crates/phy-seq-capi; carries Rust's std)
#   $out/include/phy_seq.h       its interface
#   $out/bin/openbcm-ext-macsec-rs, openbcm-ext-phy-rs   the table checkers of crates/phy-seq
#     pkgs.callPackage ./rust/sai/nix/phy-seq.nix { }
# Native builds run fmt, clippy -D warnings and the tests of the two crates. cxxTools: a
# sai-openbcm build with the C++ engines (its bin/openbcm-ext-macsec and bin/openbcm-ext-phy):
# the differential tests then compare the two implementations, over the generated tables and
# over the table files in `tables` (e.g. a board's real ones). A cross build runs nothing.
# Source: the three crates it is made of and the workspace's manifest and lock file, so edits
# to the other crates do not rebuild it (a change of the lock file does).
{
  lib,
  rustPlatform,
  stdenv,
  clippy,
  rustfmt,
  cxxTools ? null,
  tables ? [ ],
}:
let
  ws = ../.;
  manifest = lib.importTOML ../Cargo.toml;
  crates = [ "-p" "phy-seq" "-p" "phy-seq-capi" ];
in
rustPlatform.buildRustPackage {
  pname = "phy-seq-capi";
  inherit (manifest.workspace.package) version;
  src = lib.fileset.toSource {
    root = ws;
    fileset = lib.fileset.unions [
      (ws + "/Cargo.toml")
      (ws + "/Cargo.lock")
      (ws + "/clippy.toml")
      (ws + "/rustfmt.toml")
      (lib.fileset.fileFilter (f: f.hasExt "rs" || f.hasExt "h" || f.name == "Cargo.toml") (ws + "/crates/phy-seq"))
      (lib.fileset.fileFilter (f: f.hasExt "rs" || f.hasExt "h" || f.name == "Cargo.toml") (ws + "/crates/phy-seq-capi"))
      # phy-seq uses its Dev and MdioBus types; only the library sources
      (ws + "/crates/mdio-script/Cargo.toml")
      (ws + "/crates/mdio-script/src")
    ];
  };
  cargoLock.lockFile = ws + "/Cargo.lock";

  # crates/phy-seq-capi/build.rs reads include/phy_seq.h with bindgen
  nativeBuildInputs = [ rustPlatform.bindgenHook ];
  nativeCheckInputs = [ clippy rustfmt ];

  env = lib.optionalAttrs (cxxTools != null) {
    PHY_SEQ_CXX_EXT_MACSEC = "${cxxTools}/bin/openbcm-ext-macsec";
    PHY_SEQ_CXX_EXT_PHY = "${cxxTools}/bin/openbcm-ext-phy";
  } // lib.optionalAttrs (tables != [ ]) { PHY_SEQ_TABLES = lib.concatStringsSep ":" (map toString tables); };

  # as package.nix: the big-endian ppc64 ELFv2 triple the way clang spells it
  preBuild = lib.optionalString (lib.hasSuffix "gnuabielfv2" stdenv.hostPlatform.config) ''
    export BINDGEN_EXTRA_CLANG_ARGS="${"$"}{BINDGEN_EXTRA_CLANG_ARGS/--target=${stdenv.hostPlatform.config}/--target=${lib.removeSuffix "abielfv2" stdenv.hostPlatform.config} -mabi=elfv2}"
  '';

  cargoBuildFlags = crates;
  cargoTestFlags = crates;
  doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  preCheck = ''
    cargo fmt ${lib.concatStringsSep " " crates} --check
    cargo clippy --offline ${lib.concatStringsSep " " crates} --all-targets --profile "$cargoCheckType" -- -D warnings
  '';

  postInstall = ''
    install -Dm644 crates/phy-seq-capi/include/phy_seq.h $out/include/phy_seq.h
    test -s $out/lib/libphy_seq_capi.a
  '';

  meta = {
    description = "External PHY sequence tables (ext-macsec, ext-phy) in Rust as a static C library for sai-openbcm";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
