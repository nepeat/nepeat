# ai_re shared Nix: plain-Nix entry point (the flake wraps exactly this).
#   let aiRe = import ../ai_re/nix; in
#   aiRe.nixosModules.board-as7760              # module paths
#   aiRe.packagesFor pkgs                       # bootloaders etc. for a build host
#   aiRe.systemsFor { inherit nixpkgs; }        # { nixosConfigurations, packages }
#   aiRe.flakeOutputs { inherit nixpkgs; }      # what ./flake.nix and ../flake.nix export
{
  nixosModules = import ./modules;
  packagesFor = pkgs: import ./packages { inherit pkgs; };
  systemsFor = args: import ./configs args;
  # nixpkgs: a nixpkgs flake/input; rust: see ./configs/default.nix.
  flakeOutputs = { nixpkgs, buildSystem ? "x86_64-linux", rust ? null }:
    let systems = import ./configs { inherit nixpkgs buildSystem rust; };
    in {
      nixosModules = import ./modules;
      inherit (systems) nixosConfigurations;
      packages.${buildSystem} = import ./packages { pkgs = import nixpkgs { system = buildSystem; }; } // systems.packages;
    };
}
