# AS7772-32X in the lab: identity, access and management network. Hardware is
# modules/boards/as7772 (the AS7760 board + deltas), the small-appliance base
# is modules/profiles/appliance.nix. T2080 platform: ../as7760/platform.nix.
{ config, lib, pkgs, ... }:
{
  imports = [ ../../modules/profiles/appliance.nix ../../modules/boards/as7772 ];
  system.stateVersion = "26.11";
  nixpkgs.config.allowUnfreePredicate = pkg: builtins.elem (lib.getName pkg) [ "openbcm-bcm-user" "openbcm-libsdk" "sai-openbcm" ];
  networking.hostName = "as7772";
  # Base MAC from the board's ONIE TlvInfo (NOR hw-info, 0x24).
  services.sonic.macAddress = "1c:ea:0b:38:2b:d5";
  # SONiC at boot (modules/boards/as7772: sai-openbcm behind the Evora PHYs,
  # which openbcm-mdio-script brings up first from the private script in
  # /var/lib/evora-phy). Lab cabling: two 100G DACs, cage 1 <-> 2 and
  # 31 <-> 32, nothing else; every other port stays admin down (the seed).
  # FEC "rs" on one pair and none on the other: the PHY does RS-FEC on the
  # cage side either way, the ASIC side runs without (board port quirks).
  services.sonic.enable = true;
  services.sonic.settings.PORT = {
    "Ethernet1/1" = { admin_status = "up"; fec = "rs"; };
    "Ethernet2/1" = { admin_status = "up"; fec = "rs"; };
    "Ethernet31/1".admin_status = "up";
    "Ethernet32/1".admin_status = "up";
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
