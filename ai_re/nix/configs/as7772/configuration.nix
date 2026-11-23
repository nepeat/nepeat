# AS7772-32X in the lab: identity and ports. Hardware is modules/boards/as7772
# (the AS7760 board + deltas), the small-appliance base is
# modules/profiles/appliance.nix, keys and the management network are
# ../lab-access.nix. T2080 platform: ../as7760/platform.nix.
{ config, lib, pkgs, ... }:
{
  imports = [ ../lab-access.nix ../../modules/profiles/appliance.nix ../../modules/boards/as7772 ];
  system.stateVersion = "26.11";
  nixpkgs.config.allowUnfreePredicate = pkg: builtins.elem (lib.getName pkg) [ "openbcm-bcm-user" "openbcm-libsdk" ];
  networking.hostName = "as7772";
  # Base MAC from the board's ONIE TlvInfo (NOR hw-info, 0x24).
  services.sonic.macAddress = "1c:ea:0b:38:2b:d5";
  # SONiC at boot (modules/boards/as7772: the Rust SAI behind the Evora PHYs,
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
  # MACsec on both DAC loops (SONiC's own stack: macsecmgrd, MKA by
  # wpa_supplicant, SAs into the Evora PHYs through SAI;
  # devices/edge-core-as7772-32x/macsec.md "Under SONiC"). Both ends of each
  # link are this switch, so one profile serves all four ports.
  # LAB TEST KEY: the CAK file is made by hand on the switch (macsec.md), it is
  # not in this repository or the store. Without the file the profile is not
  # written and the ports stay as they are without MACsec (in clear).
  services.sonic.macsec = {
    enable = true;
    profiles.lab-test = {
      cakFile = "/var/lib/sonic-macsec/lab-test.cak";
      ckn = "6173373737322d6c61622d746573742d6b65792d323032362d31312d32320000";   # "as7772-lab-test-key-2026-11-22", public
      # cipher suite: the module's default, GCM-AES-XPN-256
    };
    ports = {
      "Ethernet1/1" = "lab-test";
      "Ethernet2/1" = "lab-test";
      "Ethernet31/1" = "lab-test";
      "Ethernet32/1" = "lab-test";
    };
  };
  # Both ends of each DAC are this switch, and every port has the switch MAC:
  # MKA cannot elect a key server between two ends with one address
  # ("duplicate MAC detected"). The far ends get the board's 2nd and 3rd
  # address (the TlvInfo assigns four from the base). Lab cabling only.
  services.sonic.settings.INTERFACE = {
    "Ethernet2/1".mac_addr = "1c:ea:0b:38:2b:d6";
    "Ethernet32/1".mac_addr = "1c:ea:0b:38:2b:d7";
  };
  # eth0 (DPAA FMan, as on the AS7760) is the management port: ../lab-access.nix.
}
