# Small cross-built NixOS for switch / embedded control planes: no docs, no
# desktop or firmware bundles, trimmed systemd, key-only sshd, bounded logs,
# a set of hardware-probing tools. Built on a workstation and deployed with
# nixos-rebuild --target-host; hosts add identity, keys and networking.
{ config, lib, pkgs, modulesPath, ... }:
{
  imports = [ (modulesPath + "/profiles/minimal.nix") ./systemd-minimal.nix ];
  i18n.supportedLocales = [ "en_US.UTF-8/UTF-8" ];

  # Mirror the useful parts of ../nix/nixos/base.nix without its desktop,
  # firmware bundle, Home Manager, indexing, agents, and build dependencies.
  environment.defaultPackages = lib.mkForce [ ];
  environment.systemPackages = with pkgs; [
    smartmontools ethtool usbutils pciutils
    iproute2 iputils tcpdump i2c-tools lm_sensors libgpiod mmc-utils
    util-linux e2fsprogs mtdutils dtc
    curl jq file strace lsof procps htop tmux vim python3
    # for flexing; see the package for why not nixpkgs' fastfetch(Minimal).
    (callPackage ../../packages/fastfetch-tiny { })
  ];
  # Wherever a profile user turns nix on, it speaks flakes.
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  documentation.enable = false;
  documentation.man.enable = false;
  documentation.nixos.enable = false;
  hardware.enableRedistributableFirmware = false;
  services.udisks2.enable = false;
  services.fwupd.enable = false;
  programs.command-not-found.enable = false;
  # Keep init/networkd/udev/logind/PAM, omit desktop/VM/secure-boot tooling.
  systemd.package = pkgs.systemd.override {
    withDocumentation = false;
    withBootloader = false;
    withUkify = false;
    withEfi = false;
    withTpm2Tss = false;
    withFido2 = false;
    withHomed = false;
    withRepart = false;
    withSysupdate = false;
    withSysinstall = false;
    withVmspawn = false;
    withNspawn = false;
    withMachined = false;
    withPortabled = false;
    withImportd = false;
    withImds = false;
    withLibBPF = false;
    vmlinux-btf = null;
    withCryptsetup = false;
    withRemote = false;
  };

  # An appliance built on the workstation. No compiler, package evaluator or
  # on-device build jobs; extra tools are added above and cross-built there.
  nix.enable = false;
  system.switch.enable = false;
  system.tools.nixos-rebuild.enable = false;
  system.tools.nixos-option.enable = false;
  system.tools.nixos-version.enable = true;

  services.openssh = {
    enable = true;
    settings = {
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
      PermitRootLogin = "prohibit-password";
    };
  };
  users.mutableUsers = false;

  # Bound SD wear and runaway logs; retain diagnostics across reboots.
  services.journald.settings.Journal = {
    Storage = "persistent";
    SystemMaxUse = "64M";
    SystemKeepFree = "1G";
    RuntimeMaxUse = "32M";
    MaxRetentionSec = "7day";
  };
  boot.tmp = { useTmpfs = true; tmpfsSize = "25%"; };
  swapDevices = [ ];
  systemd.settings.Manager = {
    DefaultTimeoutStopSec = "15s";
    RuntimeWatchdogSec = "0";
    ShutdownWatchdogSec = "0";
  };
}
