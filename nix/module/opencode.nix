{ pkgs, inputs, lib, isStandalone, ... }:
let
  commonPrompt = builtins.readFile ./common/AGENTS.md;

  # hack
  opencodePkg = inputs.opencode.packages.${pkgs.system}.default.overrideAttrs (old: {
  });

  opencodeHomeConfig = {
    home.packages = [ opencodePkg ];

    home.file.".config/opencode/AGENTS.md".text = commonPrompt;
  };
in
if isStandalone then {
  # This branch is for standalone home-manager
  home = opencodeHomeConfig.home;
} else {
  # This branch is for nix-darwin or NixOS where the home-manager module is loaded
  home-manager.sharedModules = [ opencodeHomeConfig ];
}
