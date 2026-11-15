# fand, as a callPackage-able function:
#   pkgs.callPackage ./rust/crates/fand/nix/package.nix { }
#
# The source is the Rust workspace (rust/), filtered to what cargo needs, so
# edits elsewhere in the repository, to READMEs or to these .nix files do not
# rebuild it. Tests run whenever the build machine can execute the result
# (native builds); they need nothing but /bin/sh from the sandbox.
{
  lib,
  rustPlatform,
  stdenv,
}:
let
  ws = ../../..;
  manifest = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "fand";
  inherit (manifest.package) version;

  src = lib.fileset.toSource {
    root = ws;
    fileset = lib.fileset.unions [
      (ws + "/Cargo.toml")
      (ws + "/Cargo.lock")
      (lib.fileset.fileFilter (f: f.hasExt "rs" || f.name == "Cargo.toml" || f.hasExt "yaml") (ws + "/crates"))
    ];
  };
  cargoLock.lockFile = ws + "/Cargo.lock";

  cargoBuildFlags = [ "--package" "fand" ];
  cargoTestFlags = [ "--package" "fand" ];
  doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;

  meta = {
    description = "Vendor-neutral fan control daemon for network appliances";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "fand";
  };
}
