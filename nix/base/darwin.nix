{ pkgs, lib, ... }:
{
  # Add ability to used TouchID for sudo authentication
  security.pam.services.sudo_local.touchIdAuth = lib.mkIf pkgs.stdenv.hostPlatform.isDarwin true;
}