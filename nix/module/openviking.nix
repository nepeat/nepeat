{ pkgs, lib, isStandalone, ... }:
let
  setupOpenvikingConfig = pkgs.writeShellApplication {
    name = "setup-openviking-config";
    runtimeInputs = with pkgs; [
      coreutils
      curl
      jq
      (openbao.override { withUi = false; })
    ];
    text = builtins.readFile ./openviking/setup-openviking-config.sh;
  };

  openvikingHomeConfig = {
    home.packages = [ setupOpenvikingConfig ];
  };
in
if isStandalone then {
  # This branch is for standalone home-manager
  home = openvikingHomeConfig.home;
} else {
  # This branch is for nix-darwin or NixOS where the home-manager module is loaded
  home-manager.sharedModules = [ openvikingHomeConfig ];
}
