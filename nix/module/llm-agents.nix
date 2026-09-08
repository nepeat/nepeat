{ pkgs, inputs, isStandalone, ... }:
let
  commonPrompt = builtins.readFile ./common/AGENTS.md;
  agents = inputs.llm-agents.packages.${pkgs.system};

  homeConfig = {
    home.packages = [
      agents.claude-code
      agents.codex
      agents.opencode
    ];

    home.file.".claude/CLAUDE.md".text = commonPrompt;
    home.file.".codex/AGENTS.md".text = commonPrompt;
    home.file.".config/opencode/AGENTS.md".text = commonPrompt;
  };

  # numtide's binary cache for llm-agents.nix; works for standalone home-manager
  # too since nix.settings lands in ~/.config/nix/nix.conf.
  cache = {
    nix.settings = {
      substituters = [ "https://cache.numtide.com" ];
      trusted-public-keys = [ "niks3.numtide.com-1:DTx8wZduET09hRmMtKdQDxNNthLQETkc/yaX7M4qK0g=" ];
    };
  };
in
if isStandalone then {
  # This branch is for standalone home-manager
  inherit (cache) nix;
  home = homeConfig.home;
} else {
  # This branch is for nix-darwin or NixOS where the home-manager module is loaded
  inherit (cache) nix;
  home-manager.sharedModules = [ homeConfig ];
}
