{ pkgs, lib, ... }:
{
  # nix-darwin state version
  # https://github.com/LnL7/nix-darwin/blob/master/CHANGELOG
  system.stateVersion = 5;

  # Add ability to used TouchID for sudo authentication
  security.pam.services.sudo_local.touchIdAuth = true;
}