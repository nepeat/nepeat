# erin's user account plus the home-manager config the darwin hosts and the
# standalone `homeConfigurations.erin` already share.
#
# The three module/*.nix imports go in at *system* level on purpose: each one
# branches on `isStandalone`, and the false branch (which the flake's mkNixos
# selects) registers its home config via `home-manager.sharedModules` and adds
# its binary cache to the system's nix.settings. Importing them under
# home-manager.users.erin would take the wrong branch.
{ pkgs, ... }:

{
  imports = [
    ../../module/claude.nix
    ../../module/opencode.nix
    ../../module/codex.nix
    ../../module/openviking.nix
  ];

  # base/home.nix configures zsh, so it has to be enabled at system level too
  # or it is not a valid login shell.
  programs.zsh.enable = true;
  security.sudo.wheelNeedsPassword = false;

  users.users.erin = {
    isNormalUser = true;
    description = "erin";
    shell = pkgs.zsh;
    extraGroups = [
      "wheel"
      "networkmanager"
      "video"
      "audio"
      "pipewire"
    ];
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv erin-laptop"
    ];
    hashedPassword = "!";
  };

  home-manager.users.erin = {
    imports = [
      ../../base/home.nix
      ../../machine/nonwork_home.nix
    ];
  };
}
