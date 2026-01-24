{ pkgs, lib, ... }:
{
  # nix-darwin state version
  # https://github.com/LnL7/nix-darwin/blob/master/CHANGELOG
  system.stateVersion = 5;

  # https://github.com/nix-community/home-manager/issues/4026
  users.users.nep.home = "/Users/nep";
  users.users.nepeat.home = "/Users/nepeat";

  # Nix configuration ------------------------------------------------------------------------------

  nix.enable = true;

  nix.distributedBuilds = true;

  nix.settings.substituters = [
    "https://cache.nixos.org/"
    "https://nix-community.cachix.org"
  ];

  nix.settings.trusted-public-keys = [
    "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
    "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
  ];

  nix.settings.trusted-users = [
    "@admin"
  ];

  # Enable experimental nix command and flakes
  # nix.package = pkgs.nixUnstable;
  nix.extraOptions = ''
    auto-optimise-store = false
    experimental-features = nix-command flakes
  '' + lib.optionalString (pkgs.stdenv.hostPlatform.system == "aarch64-darwin") ''
    extra-platforms = x86_64-darwin aarch64-darwin
  '';

  # Create /etc/bashrc that loads the nix-darwin environment.
  programs.bash.enable = true;
  programs.zsh.enable = true;

  # https://github.com/nix-community/home-manager/issues/423
  programs.nix-index.enable = true;

  # Fonts
  fonts.packages = with pkgs; [
     nerd-fonts.recursive-mono
     nerd-fonts.jetbrains-mono
     nerd-fonts.comic-shanns-mono
     mona-sans
   ];

  # Add ability to used TouchID for sudo authentication
  security.pam.services.sudo_local.touchIdAuth = true;
}
