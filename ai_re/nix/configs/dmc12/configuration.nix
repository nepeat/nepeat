# Generic DMC-12-48R-AS0 system: one image for every unit, no per-box
# identity baked in (host keys are generated on first boot). Hardware is
# modules/boards/dmc12, the appliance base is modules/profiles/appliance.nix,
# root keys and the management network are ../lab-access.nix.
{ config, lib, pkgs, ... }:
{
  imports = [ ../lab-access.nix ../../modules/profiles/appliance.nix ../../modules/boards/dmc12 ];
  system.stateVersion = "26.11";
  networking.hostName = lib.mkDefault "dmc12";
  # Lab wiring of u01 (labels are also accepted as port names: `ssh -t console@dmc12 cs8210`).
  services.consoled.ports = {
    P1 = { label = "cs8210"; baud = 115200; };
    P48 = { label = "as7760"; baud = 115200; };
  };
  # Consoles: root's keys (../lab-access.nix) plus, for consoles only, the owner's desktop user, so `ssh -t console@dmc12 …` needs no sudo.
  services.consoled.ssh.authorizedKeys = config.users.users.root.openssh.authorizedKeys.keys ++ [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIK2WWFPDogbCuq7ta6lvKlfPqRXOJE1R3e8Vv2r+l9Fi erin@sea420-desktop"
  ];

  environment.systemPackages = [ pkgs.picocom pkgs.tio ];
  # The NIC driver is the Rust al_eth (packages/al-eth/rust-module.nix; Rust-enabled kernel with the
  # net bindings). Promoted on u01 2026-11-23 (system-26) after a trial boot and a 3 h soak of this
  # exact system on the release kernel (devices/dmc-12-48r-as0/al-eth-rs-hw-20261123.txt,
  # soak-al-eth-rs-20261123.txt); the C module stays available as ethDriver = "c" (system-25 = rollback).
  hardware.alpineV2.ethDriver = "rust";
  # One-shot trial boots with automatic revert (modules/profiles/trial-boot): the verifier pings the
  # gateway and the lab workstation. Only where our U-Boot's standard boot reads the disk (not the
  # netboot system, not the chainload layout).
  system.trialBoot = {
    enable = lib.mkDefault ((config.hardware.dmc12.bootLoader or null) == "standard");
    verify.hosts = [ "10.60.2.208" ];
  };
  # eth0 (al_eth, RGMII to the uplink PHY pair: 1G copper or SFP) is the management port: ../lab-access.nix.
}
