{ pkgs, lib, ... }:
{
  # nix-darwin state version
  # https://github.com/LnL7/nix-darwin/blob/master/CHANGELOG
  system.stateVersion = 5;

  # https://github.com/nix-community/home-manager/issues/4026
  users.users.nep.home = "/Users/nep";
  users.users.nepeat.home = "/Users/nepeat";

  # Add ability to used TouchID for sudo authentication
  security.pam.services.sudo_local.touchIdAuth = true;

  # Set SSH signer to be macOS 1Password
  programs.git.settings = {
    "gpg \"ssh\"" = lib.mkDefault {
        program = "/Applications/1Password.app/Contents/MacOS/op-ssh-sign";
    };
  };
}
