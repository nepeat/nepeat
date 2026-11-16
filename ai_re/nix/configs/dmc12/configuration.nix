# Generic DMC-12-48R-AS0 system: one image for every unit, no per-box
# identity baked in (host keys are generated on first boot). Hardware is
# modules/boards/dmc12, the appliance base is modules/profiles/appliance.nix.
{ config, lib, pkgs, ... }:
{
  imports = [ ../../modules/profiles/appliance.nix ../../modules/boards/dmc12 ];
  system.stateVersion = "26.11";
  networking.hostName = lib.mkDefault "dmc12";
  # Lab wiring of u01 (labels are also accepted as port names: `ssh -t console@dmc12 cs8210`).
  services.consoled.ports = {
    P1 = { label = "cs8210"; baud = 115200; };
    P48 = { label = "as7760"; baud = 115200; };
  };
  time.timeZone = "America/Los_Angeles";
  # Same root keys as the AS7760 (https://github.com/nepeat.keys + the build host).
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBdCw3FOLZYLg9E9VSbJYhFFFOFYOS+F0Fzi7PZPW+tv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII1xDf7IxdQU/hIvDYL6VCzfV14ZbfyLxJnIYFsBHqxI"
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBI6/FNMHDsQ/b6I2vjOeTyyQh02kdSDKRR2FQHGe72DLwypxHO7QTYnhVs9q5GRsFRC0Dj8zwBg+ve/9Ny3I2b0="
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCkEDEKqOyMe6QHZ9stWPbm+lE+NKoEer6BRW8LmEAZ7ekxjY+OiBmh0sBLMBbeOMannm7fvQ27bShZCnglDaW9k5Rf2d7vdmRBbvGkNjvg6nbFJT6M64yi8jxPt2FixDhrkcDlFUbKCZMGEJ6z5vSHm5pqDnRXdse3KPQ+Ul8h5pSfM31Nubto4onhhu0xfFuLorxUFD1/opvLs9y/geqQQ8T+zu5Ms05MfVvGD1S+Uo+EieOnhore7KdcfhG4Fi0AkwpfVVB91DS6SVSQJSbVIKLo366PVEl9JdAn59ZvaEOxq70yW36YOIRAFAm1ANLYj3jhbxf68ds7+RjgOqXH"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA60KDLPhZaDPbstXgHVK4RnZq3dSzfbVzfVBheWOMb+ root@sea420-desktop"
  ];
  # Consoles only (not root): the owner's desktop user, so `ssh -t console@dmc12 …` needs no sudo.
  services.consoled.ssh.authorizedKeys = config.users.users.root.openssh.authorizedKeys.keys ++ [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIK2WWFPDogbCuq7ta6lvKlfPqRXOJE1R3e8Vv2r+l9Fi erin@sea420-desktop"
  ];

  services.getty.autologinUser = "root";
  environment.systemPackages = [ pkgs.picocom pkgs.tio ];

  networking.useDHCP = false;
  networking.useNetworkd = true;
  networking.usePredictableInterfaceNames = false;
  systemd.network = {
    enable = true;
    wait-online.enable = false;
    # eth0: al_eth, RGMII to the uplink PHY pair (1G copper or SFP).
    networks."10-management" = {
      matchConfig.Name = "eth0";
      networkConfig = { DHCP = "yes"; IPv6AcceptRA = true; };
      dhcpV4Config.ClientIdentifier = "mac";
      linkConfig.RequiredForOnline = "no";
    };
  };
  networking.firewall.allowedTCPPorts = [ 22 ];
  networking.nftables.enable = true;
}
