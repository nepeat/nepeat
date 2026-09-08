{ pkgs, inputs, lib, isStandalone, ... }:
let
  commonPrompt = builtins.readFile ./common/AGENTS.md;

  claudeHomeConfig = {
    home.packages = [ inputs.llm-agents.packages.${pkgs.system}.claude-code ];

    home.file.".claude/CLAUDE.md".text = commonPrompt;
  };
in
if isStandalone then {
  # This branch is for standalone home-manager
  home = claudeHomeConfig.home;
} else {
  # This branch is for nix-darwin or NixOS where the home-manager module is loaded.
  # The binary cache lives in base/configuration.nix and nixos/base.nix now that
  # all three agents share one upstream.
  home-manager.sharedModules = [ claudeHomeConfig ];
}
