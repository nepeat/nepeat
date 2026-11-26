# lab-power as a callPackage-able function (the ai_re devshell: ai_re/flake.nix):
#   pkgs.callPackage ./rust/lab-power/nix/package.nix { }
# Source: this workspace only (its own lock file), filtered to what cargo needs (sources, the
# built-in targets.json and the test fixtures), so edits elsewhere, to the README or to this file
# do not rebuild it. The check runs fmt, clippy -D warnings and the tests (fake Home Assistant
# server on loopback, fake ssh on PATH: no network, no hardware). ssh comes from openssh.
{
  lib,
  rustPlatform,
  stdenv,
  makeWrapper,
  openssh,
  clippy,
  rustfmt,
}:
let
  ws = ../.;
  manifest = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "lab-power";
  inherit (manifest.package) version;
  src = lib.fileset.toSource {
    root = ws;
    fileset = lib.fileset.unions [
      (ws + "/Cargo.toml")
      (ws + "/Cargo.lock")
      (ws + "/clippy.toml")
      (ws + "/rustfmt.toml")
      (ws + "/targets.json")
      (ws + "/src")
      (ws + "/tests")
    ];
  };
  cargoLock.lockFile = ws + "/Cargo.lock";
  nativeBuildInputs = [ makeWrapper ];
  nativeCheckInputs = [ clippy rustfmt ];
  doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  preCheck = ''
    cargo fmt --all --check
    cargo clippy --offline --all-targets --profile "$cargoCheckType" -- -D warnings
  '';
  postInstall = ''
    wrapProgram $out/bin/lab-power --prefix PATH : ${lib.makeBinPath [ openssh ]}
  '';
  meta = {
    description = "Lab power control: Home Assistant plugs and the NetBotz 250 switched outlet";
    license = lib.licenses.mit;
    platforms = lib.platforms.unix;
    mainProgram = "lab-power";
  };
}
