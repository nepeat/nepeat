# Shared NixOS base for every host in this repo.
#
# Deliberately lean: this is the personal fleet, so none of the
# generalprogramming infrastructure that the megarepo's base.nix carried
# (Attic cache on fmt2, ssh-ca, nixos-tags, netbox-driven dns, comin gitops,
# the pinned glances package) came along. Add things here only when *both*
# a fat desktop and a diskless thin client actually want them.
{
  lib,
  pkgs,
  inputs,
  self,
  ...
}:

{
  nixpkgs.hostPlatform = lib.mkDefault "x86_64-linux";
  nixpkgs.config.allowUnfree = true;

  # disko from the flake input rather than whatever nixpkgs pinned, so the
  # CLI matches the module that generated the layout.
  nixpkgs.overlays = [
    (final: prev: {
      disko = inputs.disko.packages.${prev.stdenv.hostPlatform.system}.default;
    })
  ];

  nix.settings = {
    experimental-features = [
      "nix-command"
      "flakes"
    ];
    substituters = [
      "https://cache.nixos.org/"
      "https://nix-community.cachix.org"
      # Prebuilt claude-code / opencode / codex from numtide's llm-agents.nix.
      # Only hits because that input does NOT follow our nixpkgs -- see flake.nix.
      "https://cache.numtide.com"
    ];
    trusted-public-keys = [
      "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
      "niks3.numtide.com-1:DTx8wZduET09hRmMtKdQDxNNthLQETkc/yaX7M4qK0g="
    ];
  };

  environment.sessionVariables.FLAKE = self;

  boot.loader.systemd-boot = {
    enable = lib.mkDefault true;
    consoleMode = "auto";
  };
  boot.loader.efi.canTouchEfiVariables = lib.mkDefault true;

  systemd.settings.Manager = {
    # Don't wait too long for services to stop:
    DefaultTimeoutStopSec = "15s";
    # Prevent the system from hanging:
    RuntimeWatchdogSec = "5m";
    ShutdownWatchdogSec = "15m";
  };

  services.openssh = {
    enable = true;
    settings = {
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
    };
    openFirewall = true;
  };

  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv erin-laptop"
  ];

  services.journald.settings.Journal = {
    SystemMaxUse = "2G";
    MaxRetentionSec = "3month";
  };

  hardware.enableRedistributableFirmware = true;
  hardware.cpu.intel.updateMicrocode = true;
  hardware.cpu.amd.updateMicrocode = true;

  time.timeZone = "America/Los_Angeles";

  # LLDP so hosts announce themselves to the switch.
  services.lldpd.enable = true;

  environment.systemPackages = with pkgs; [
    curl
    dig
    disko
    dmidecode
    git
    htop
    jq
    mtr
    tmux
    vim
    wget
  ];

  # comma + command-not-found without a local index build.
  programs.nix-index.enable = true;
  programs.nix-index-database.comma.enable = true;

  programs.direnv = {
    enable = true;
    nix-direnv.enable = true;
  };

  # home-manager is loaded on every host by the flake's mkNixos; hosts that
  # define no users just get an empty one. Global pkgs so home configs share
  # the system's nixpkgs and overlays rather than instantiating their own.
  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;

  imports = [ ./module/remote-builders.nix ];
}
