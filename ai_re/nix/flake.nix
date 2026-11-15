{
  description = "ai_re shared Nix: packages, NixOS modules and system configs for reverse-engineered hardware";
  # Pinned separately from the ai_re devshell: these systems track nixos-unstable.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }: let
    buildSystem = "x86_64-linux";
    aiRe = import ./.;
    # No fand here: a path flake cannot reach ../../rust. The ai_re root flake
    # (a git flake over the whole monorepo) passes it; see ../flake.nix.
    systems = aiRe.systemsFor { inherit nixpkgs buildSystem; };
  in {
    inherit (aiRe) nixosModules;
    inherit (systems) nixosConfigurations;
    packages.${buildSystem} = aiRe.packagesFor (import nixpkgs { system = buildSystem; }) // systems.packages;
  };
}
