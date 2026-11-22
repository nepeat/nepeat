# AS7772-32X in the lab: identity and ports. Hardware is modules/boards/as7772
# (the AS7760 board + deltas), the small-appliance base is
# modules/profiles/appliance.nix, keys and the management network are
# ../lab-access.nix. T2080 platform: ../as7760/platform.nix.
{ config, lib, pkgs, ... }:
{
  imports = [ ../lab-access.nix ../../modules/profiles/appliance.nix ../../modules/boards/as7772 ];
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
  # eth0 (DPAA FMan, as on the AS7760) is the management port: ../lab-access.nix.
}
