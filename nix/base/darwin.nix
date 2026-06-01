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

  # not likely that i'll ever use an intel mac again
  nixpkgs.hostPlatform = "aarch64-darwin";

  # remote builder for arm64 linux
  nix.buildMachines = [{
    hostName = "dreamflasher.skate-gopher.ts.net";
    sshUser = "root";
    sshKey = "/Users/nep/.ssh/id_ed25519";
    system = "aarch64-linux";
    protocol = "ssh-ng";
    maxJobs = 8;
    supportedFeatures = [ "nixos-test" "benchmark" "big-parallel" ];
    mandatoryFeatures = [ ];
  }];
}
