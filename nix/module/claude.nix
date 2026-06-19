{ pkgs, inputs, lib, isStandalone, ... }:
let
  commonPrompt = builtins.readFile ./common/AGENTS.md;

  claudeHomeConfig = {
    home.packages = [ inputs.claude-code.packages.${pkgs.system}.default ];

    home.file.".claude/CLAUDE.md".text = commonPrompt;
  };
in
if isStandalone then {
  # This branch is for standalone home-manager
  home = claudeHomeConfig.home;
} else {
  # This branch is for nix-darwin or NixOS where the home-manager module is loaded
  nix.settings = {
    substituters = [ "https://claude-code.cachix.org" ];
    trusted-public-keys = [ "claude-code.cachix.org-1:YeXf2aNu7UTX8Vwrze0za1WEDS+4DuI2kVeWEE4fsRk=" ];
  };
  home-manager.sharedModules = [ claudeHomeConfig ];
}
