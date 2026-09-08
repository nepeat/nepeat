{ inputs, pkgs, ... }:
{
  imports = [ inputs.paseo.nixosModules.paseo ];

  services.paseo = {
    enable = true;

    user = "erin";
    group = "users";

    relay.enable = true;

    environment.PASEO_WEB_UI_ENABLED = "false";
    settings.features.webUi.enabled = false;
  };

  environment.systemPackages = [ inputs.paseo.packages.${pkgs.stdenv.hostPlatform.system}.default ];
}
