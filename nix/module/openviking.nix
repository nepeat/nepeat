{ pkgs, lib, inputs, isStandalone, ... }:
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
    # Provides `ov`, `openviking` and `openviking-server`.
    home.packages = [
      setupOpenvikingConfig
      inputs.self.packages.${pkgs.system}.openviking
    ];
  };
in
if isStandalone then {
  # This branch is for standalone home-manager
  home = openvikingHomeConfig.home;
} else {
  # This branch is for nix-darwin or NixOS where the home-manager module is loaded
  home-manager.sharedModules = [ openvikingHomeConfig ];
}
