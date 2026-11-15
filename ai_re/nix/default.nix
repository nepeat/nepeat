# ai_re shared Nix: plain-Nix entry point (the flake wraps exactly this).
#   let aiRe = import ../ai_re/nix; in
#   aiRe.nixosModules.board-as7760              # module paths
#   aiRe.packagesFor pkgs                       # bootloaders etc. for a build host
#   aiRe.systemsFor { inherit nixpkgs; }        # { nixosConfigurations, packages }
{
  nixosModules = import ./modules;
  packagesFor = pkgs: import ./packages { inherit pkgs; };
  systemsFor = args: import ./configs args;
}
