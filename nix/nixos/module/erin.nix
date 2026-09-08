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
    # SSH-key-only. "!" is the documented way to say "this account has no
    # password"; it blocks password logins outright while leaving key auth
    # working, and unlike an absent hashedPasswordFile it is an explicit
    # statement rather than an activation-time warning.
    #
    # Note `passwd erin` will NOT stick on this host: impermanence wipes /etc,
    # so /etc/shadow is rebuilt every boot. To get a console password later,
    # put the hash on /persist and swap which of these two lines is live:
    #   mkpasswd -m yescrypt | sudo tee /persist/passwords/erin
    hashedPassword = "!";
    # hashedPasswordFile = "/persist/passwords/erin";
  };

  home-manager.users.erin = {
    imports = [
      ../../base/home.nix
      ../../machine/nonwork_home.nix
    ];
  };

  # Deliberately not imported here:
  #   base/non_nix_home.nix   -- its own comment scopes it to standalone
  #                              (non-NixOS) home configs; it also sets
  #                              nix.package = pkgs.lix at the home level,
  #                              which fights a NixOS system's own nix.
  #   base/configuration.nix  -- system-level nix settings that nixos/base.nix
  #                              already covers.
}
