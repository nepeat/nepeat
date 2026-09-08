# sea420-desktop -- the big AMD desktop. ZFS-on-LUKS root with impermanence,
# Plasma 6, gaming, and a couple of small network services for the lab.
#
# Moved here from the generalprogramming megarepo. Three of its old imports did
# not come with it: `dns` (netbox/ipa/consul-specific), `glances-tty` (needs
# the org's pinned glances package), and `gitops` (comin, driven off the
# megarepo's vars/machines.nix -- and it was already `enable = false`).
{
  config,
  pkgs,
  ...
}:

{
  system.stateVersion = "26.05";

  imports = [
    ../../module/impermanence.nix
    ../../module/nvidia.nix
    ../../module/erin.nix
    ../../module/paseo.nix

    ./hardware.nix
    ./boot.nix
    ./disko.nix
    ./nfs.nix
    ./tftp.nix
  ];

  networking = {
    domain = "generalprogramming.org";
    hostId = "30b7aad6";
    useDHCP = true;
  };

  # Impermanence wipes /etc every boot, so machine-id has to be pinned rather
  # than generated. This is the value the host has been running with since the
  # megarepo -- changing it would orphan its journal and any machine-id-scoped
  # state. The kernel param covers stage 1, before /etc exists.
  environment.etc.machine-id.text = "30b7aad6952aeda770f149286983149d";
  boot.kernelParams = [ "systemd.machine_id=30b7aad6952aeda770f149286983149d" ];

  # Latest kernel for hardware support; build the zfs_unstable kernel module
  # against it even when newer than ZFS officially supports. The `broken` gate
  # is on the module (not userland), so we override modulePackage. Note: the
  # intended `.override { enableUnsupportedExperimentalKernel = true; }` is
  # silently dropped here (unstable.nix swallows it via `...`@args), so patch
  # the derivation directly: add the configure flag and clear `broken`.
  boot = {
    kernelPackages = pkgs.linuxPackages_latest;
    zfs.package = pkgs.zfs_unstable;
    zfs.modulePackage = config.boot.kernelPackages.zfs_unstable.overrideAttrs (old: {
      configureFlags = (old.configureFlags or [ ]) ++ [ "--enable-linux-experimental" ];
      meta = old.meta // {
        broken = false;
      };
    });
  };

  nixpkgs.config.problems.handlers.zfs.broken = "ignore";

  # Enable impermanence via /persist:
  impermanence.enable = true;
  fileSystems."/persist".neededForBoot = true;

  # Enable podman
  virtualisation = {
    containers.enable = true;
    podman = {
      enable = true;
      dockerCompat = true;
    };
    oci-containers.backend = "podman";
  };

  # punch holes for ports we might play with
  networking.firewall.allowedTCPPorts = [
    8000
    8080
  ];

  environment.systemPackages = with pkgs; [
    # other useful tools
    gnumake
    ubootTools
    screen
    # useful dev tools
    devenv
    python315
    tcpdump
    # human interactions with podman
    podman
    # flexing
    fastfetch
    # KDE
    kdePackages.discover # Optional: Install if you use Flatpak or fwupd firmware update sevice
    kdePackages.kcalc # Calculator
    kdePackages.kcharselect # Tool to select and copy special characters from all installed fonts
    kdePackages.kclock # Clock app
    kdePackages.kcolorchooser # A small utility to select a color
    kdePackages.kolourpaint # Easy-to-use paint program
    kdePackages.ksystemlog # KDE SystemLog Application
    kdePackages.sddm-kcm # Configuration module for SDDM
    kdiff3 # Compares and merges 2 or 3 files or directories
    kdePackages.isoimagewriter # Optional: Program to write hybrid ISO files onto USB disks
    kdePackages.partitionmanager # Optional: Manage the disk devices, partitions and file systems on your computer
    # Non-KDE graphical packages
    hardinfo2 # System information and benchmarks for Linux systems
    vlc # Cross-platform media player and streaming server
    wayland-utils # Wayland utilities
    wl-clipboard # Command-line copy/paste utilities for Wayland
    chromium
    gamescope-wsi
    steam-run
  ];

  programs.firefox.enable = true;

  # Desktop
  services.desktopManager.plasma6.enable = true;

  # Default display manager for Plasma
  services.displayManager.sddm = {
    enable = true;
    # To use Wayland (Experimental for SDDM)
    wayland.enable = true;
  };

  # Audio
  security.rtkit.enable = true;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
    systemWide = true;
  };

  # Gaming
  programs.steam = {
    enable = true;
    remotePlay.openFirewall = true; # Open ports in the firewall for Steam Remote Play
    dedicatedServer.openFirewall = true; # Open ports for Source Dedicated Server hosting
  };

  programs.gamescope = {
    enable = true;
    capSysNice = true;
  };

  programs.gamemode.enable = true;

  # Users
  users.users.meow = {
    isNormalUser = true;
    extraGroups = [
      "wheel"
      "networkmanager"
      "video"
      "audio"
      "pipewire"
    ];
    hashedPasswordFile = "/persist/passwords/meow";
  };

  # Disable auto sleep
  systemd.sleep.settings.Sleep = {
    AllowSuspend = "no";
    AllowHibernation = "no";
    AllowHybridSleep = "no";
    AllowSuspendThenHibernate = "no";
  };

  # Enable tailscale for this host only
  services.tailscale = {
    enable = true;
    useRoutingFeatures = "server";
  };

  # Allow other arch emulation
  boot.binfmt.emulatedSystems = [ "aarch64-linux" ];
  boot.binfmt.preferStaticEmulators = true;
  boot.binfmt.registrations."aarch64-linux".fixBinary = true;
}
