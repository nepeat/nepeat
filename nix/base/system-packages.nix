{ pkgs, lib, ... }:
{
  # Apps
  # `home-manager` currently has issues adding them to `~/Applications`
  # Issue: https://github.com/nix-community/home-manager/issues/1341
  environment.systemPackages = with pkgs; [
    terminal-notifier
    file

    # hsm + smartcard stuff
    openssl
    libp11
    opensc

    # prefer devenv in the system
    devenv

    # misc
    wireguard-tools
  ];

  # Fonts
  fonts.packages = with pkgs; [
     nerd-fonts.recursive-mono
     nerd-fonts.jetbrains-mono
     nerd-fonts.comic-shanns-mono
     mona-sans
   ];
}
