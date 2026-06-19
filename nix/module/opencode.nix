{ pkgs, inputs, lib, isStandalone, ... }:
let
  commonPrompt = builtins.readFile ./common/AGENTS.md;

  # Relax the Bun version check from a hard error to a warning, matching the
  # official nixpkgs opencode package.
  opencodePkg = inputs.opencode.packages.${pkgs.system}.default.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      substituteInPlace packages/script/src/index.ts \
        --replace-fail 'throw new Error(`This script requires bun@''${expectedBunVersionRange}' \
                       'console.warn(`Warning: This script requires bun@''${expectedBunVersionRange}'
    '';
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
