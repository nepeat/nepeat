{ pkgs, lib, ... }:
{
  # nix-darwin state version
  # https://github.com/LnL7/nix-darwin/blob/master/CHANGELOG
  system.stateVersion = 5;

  # https://github.com/nix-community/home-manager/issues/4026
  users.users.nep.home = "/Users/nep";

  # Nix configuration ------------------------------------------------------------------------------

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

  nix.configureBuildUsers = true;

  # Enable experimental nix command and flakes
  # nix.package = pkgs.nixUnstable;
  nix.extraOptions = ''
    auto-optimise-store = false
    experimental-features = nix-command flakes repl-flake
  '' + lib.optionalString (pkgs.system == "aarch64-darwin") ''
    extra-platforms = x86_64-darwin aarch64-darwin
  '';

  # Create /etc/bashrc that loads the nix-darwin environment.
  programs.bash.enable = true;
  programs.zsh.enable = true;

  # Auto upgrade nix package and the daemon service.
  services.nix-daemon.enable = true;

  # Apps
  # `home-manager` currently has issues adding them to `~/Applications`
  # Issue: https://github.com/nix-community/home-manager/issues/1341
  environment.systemPackages = with pkgs; [
    terminal-notifier
    file

    # fuck macos
    gnutar
    bash

    # global tools
    pkg-config

    # hsm + smartcard stuff
    openssl
    libp11

    # python
    python312

    # misc
    wireguard-tools
  ];

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
  security.pam.enableSudoTouchIdAuth = true;
}
