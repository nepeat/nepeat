# cs8210-optics + cs8210-leds, as a callPackage-able function:
#   pkgs.callPackage ./rust/cs8210/nix/package.nix { }
# Source: this workspace only (its own lock file), filtered to what cargo needs, so
# edits elsewhere in the repository, to READMEs or to these .nix files do not rebuild
# it. Tests run whenever the build machine can execute the result (native and the
# x86_64 -> aarch64 cross build's test-only native variant: see passthru.tests).
{
  lib,
  rustPlatform,
  stdenv,
}:
let
  ws = ../.;
  manifest = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "cs8210-platform";
  inherit (manifest.workspace.package) version;
  src = lib.fileset.toSource {
    root = ws;
    fileset = lib.fileset.unions [
      (ws + "/Cargo.toml")
      (ws + "/Cargo.lock")
      (ws + "/clippy.toml")
      (lib.fileset.fileFilter (f: f.hasExt "rs" || f.name == "Cargo.toml") (ws + "/crates"))
    ];
  };
  cargoLock.lockFile = ws + "/Cargo.lock";
  doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  meta = {
    description = "CS8210-32X platform: the I2C FPGA owner (optics) and the LED CPLD tool";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "cs8210-optics";
  };
}
