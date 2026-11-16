# consoled + console, as a callPackage-able function:
#   pkgs.callPackage ./rust/consoled/nix/package.nix { }
# Source: this crate only (its own workspace and lock file), so edits elsewhere in the
# repository, to READMEs or to these .nix files do not rebuild it. Tests run on native
# builds (they need openpty, nothing else).
{
  lib,
  rustPlatform,
  stdenv,
}:
let
  crate = ../.;
  manifest = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "consoled";
  inherit (manifest.package) version;
  src = lib.fileset.toSource {
    root = crate;
    fileset = lib.fileset.unions [
      (crate + "/Cargo.toml")
      (crate + "/Cargo.lock")
      (crate + "/src")
      (crate + "/tests")
    ];
  };
  cargoLock.lockFile = crate + "/Cargo.lock";
  doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  meta = {
    description = manifest.package.description;
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "consoled";
  };
}
