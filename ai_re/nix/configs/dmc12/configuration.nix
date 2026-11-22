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
  # eth0 (al_eth, RGMII to the uplink PHY pair: 1G copper or SFP) is the management port: ../lab-access.nix.
}
