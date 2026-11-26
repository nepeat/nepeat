# The ifcs-gen program (crates/ifcs-gen, feature cli: sai-teralynx's gen_ifcs_api.py in Rust) for the
# platform it is called for (./ifcs-headers.nix: buildPackages). The crate has no dependencies, so
# rustc builds it directly (no cargo, no vendored workspace): only its own files are inputs and
# nothing else of the workspace rebuilds it. Its tests, clippy and fmt run in ./package.nix.
#   pkgs.buildPackages.callPackage ./ifcs-gen.nix { }
{
  lib,
  stdenv,
  rustc,
}:
let
  crate = ../crates/ifcs-gen;
  manifest = lib.importTOML ../Cargo.toml;
in
stdenv.mkDerivation {
  pname = "ifcs-gen";
  inherit (manifest.workspace.package) version;
  src = lib.fileset.toSource {
    root = crate;
    fileset = lib.fileset.unions [ (crate + "/src") (crate + "/templates") ];
  };
  nativeBuildInputs = [ rustc ];
  buildPhase = ''
    runHook preBuild
    flags="--edition ${manifest.workspace.package.edition} -C opt-level=2"
    rustc $flags --crate-type rlib --crate-name ifcs_gen src/lib.rs -o libifcs_gen.rlib
    rustc $flags --cfg 'feature="cli"' --crate-type bin --crate-name ifcs_gen_cli src/main.rs \
      --extern ifcs_gen=libifcs_gen.rlib -o ifcs-gen
    runHook postBuild
  '';
  installPhase = ''
    install -Dm755 ifcs-gen $out/bin/ifcs-gen
  '';
  meta = {
    description = "The IFCS header generator in Rust (crates/ifcs-gen)";
    license = lib.licenses.asl20;
    mainProgram = "ifcs-gen";
  };
}
