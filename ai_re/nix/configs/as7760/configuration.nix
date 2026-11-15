# AS7760-32X in the lab: identity, access and management network. Hardware is
# modules/boards/as7760, the small-appliance base is modules/profiles/appliance.nix.
{ config, lib, pkgs, ... }:
{
  imports = [ ../../modules/profiles/appliance.nix ../../modules/boards/as7760 ];
  system.stateVersion = "26.11";
  # The Broadcom SDK is source-available, not free software.
  nixpkgs.config.allowUnfreePredicate = pkg: builtins.elem (lib.getName pkg) [ "openbcm-bcm-user" "openbcm-libsdk" "sai-openbcm" ];
  networking.hostName = "as7760";
  # SONiC switch stack (modules/boards/as7760 wires it to sai-openbcm); on at
  # boot since 2026-11-15 (generated config.bcm, kernel with bridge/VLAN/team).
  services.sonic.enable = true;
  # Base MAC from the board's ONIE EEPROM (TlvInfo 0x24).
  services.sonic.macAddress = "8c:ea:1b:89:c9:3c";
  # Lab loop: a QSFP28 4x25G breakout cable from cage 1 whose first leg sits
  # in cage 32 through a QSFP-to-SFP28 adapter (QSA, lane 1), so both cages
  # are split 4x25G (every subport listed, config-ownership.md "ports"; the
  # SDK config follows from these).
  services.sonic.breakout = {
    "1" = [
      { alias = "Ethernet1/1"; lanes = [ 49 ]; speed = 25000; }
      { alias = "Ethernet1/2"; lanes = [ 50 ]; speed = 25000; }
      { alias = "Ethernet1/3"; lanes = [ 51 ]; speed = 25000; }
      { alias = "Ethernet1/4"; lanes = [ 52 ]; speed = 25000; }
    ];
    "32" = [
      { alias = "Ethernet32/1"; lanes = [ 13 ]; speed = 25000; }
      { alias = "Ethernet32/2"; lanes = [ 14 ]; speed = 25000; }
      { alias = "Ethernet32/3"; lanes = [ 15 ]; speed = 25000; }
      { alias = "Ethernet32/4"; lanes = [ 16 ]; speed = 25000; }
    ];
  };
  time.timeZone = "America/Los_Angeles";
  # Same root keys as tn48m/cs8210: https://github.com/nepeat.keys (fetched
  # 2026-11-09) plus root on sea420-desktop, the build/deploy host.
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMVk9i7FG7dc9r4ixwAJT7uPLH3UuqbwIgeZ7Ytmnpvv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBdCw3FOLZYLg9E9VSbJYhFFFOFYOS+F0Fzi7PZPW+tv"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII1xDf7IxdQU/hIvDYL6VCzfV14ZbfyLxJnIYFsBHqxI"
    "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBI6/FNMHDsQ/b6I2vjOeTyyQh02kdSDKRR2FQHGe72DLwypxHO7QTYnhVs9q5GRsFRC0Dj8zwBg+ve/9Ny3I2b0="
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCkEDEKqOyMe6QHZ9stWPbm+lE+NKoEer6BRW8LmEAZ7ekxjY+OiBmh0sBLMBbeOMannm7fvQ27bShZCnglDaW9k5Rf2d7vdmRBbvGkNjvg6nbFJT6M64yi8jxPt2FixDhrkcDlFUbKCZMGEJ6z5vSHm5pqDnRXdse3KPQ+Ul8h5pSfM31Nubto4onhhu0xfFuLorxUFD1/opvLs9y/geqQQ8T+zu5Ms05MfVvGD1S+Uo+EieOnhore7KdcfhG4Fi0AkwpfVVB91DS6SVSQJSbVIKLo366PVEl9JdAn59ZvaEOxq70yW36YOIRAFAm1ANLYj3jhbxf68ds7+RjgOqXH"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA60KDLPhZaDPbstXgHVK4RnZq3dSzfbVzfVBheWOMb+ root@sea420-desktop"
  ];
  services.getty.autologinUser = "root";

  networking.useDHCP = false;
  networking.useNetworkd = true;
  networking.usePredictableInterfaceNames = false;
  systemd.network = {
    enable = true;
    wait-online.enable = false;
    # eth0 is the management interface in the captured ONIE environment.
    # Confirm its modern DPAA driver binding before relying on remote access.
    networks."10-management" = {
      matchConfig.Name = "eth0";
      networkConfig = { DHCP = "yes"; IPv6AcceptRA = true; };
      dhcpV4Config.ClientIdentifier = "mac";
      linkConfig.RequiredForOnline = "no";
    };
  };
  # 2026-11-14: the old "ethtool -K eth0 rxhash off" pre-start workaround was
  # removed. Register reads showed that toggle never bypasses KeyGen (BMI
  # fmbm_rfpne stays NIA_ENG_HWK; it only stops skb_set_hash), and RX with
  # hashing on works under full NixOS. See linux-bringup.md before re-adding.
  networking.firewall.allowedTCPPorts = [ 22 ];
  networking.nftables.enable = true;

}
