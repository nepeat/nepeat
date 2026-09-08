{ pkgs, inputs, lib, isStandalone, ... }:
let
  commonPrompt = builtins.readFile ./common/AGENTS.md;

  codexHomeConfig = {
    home.packages = [ inputs.llm-agents.packages.${pkgs.system}.codex ];

    # Codex reads global instructions from ~/.codex/AGENTS.md, the same way
    # opencode reads ~/.config/opencode/AGENTS.md and claude reads
    # ~/.claude/CLAUDE.md. Same prompt file behind all three.
    home.file.".codex/AGENTS.md".text = commonPrompt;
  };
in
if isStandalone then {
  # This branch is for standalone home-manager
  home = codexHomeConfig.home;
} else {
  # This branch is for nix-darwin or NixOS where the home-manager module is loaded
  home-manager.sharedModules = [ codexHomeConfig ];
}
