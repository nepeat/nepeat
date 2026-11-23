# AS7760-32X in the lab: identity and ports. Hardware is modules/boards/as7760,
# the small-appliance base is modules/profiles/appliance.nix, keys and the
# management network are ../lab-access.nix.
{ config, lib, pkgs, ... }:
{
  imports = [ ../lab-access.nix ../../modules/profiles/appliance.nix ../../modules/boards/as7760 ];
  system.stateVersion = "26.11";
  # The Broadcom SDK is source-available, not free software.
  nixpkgs.config.allowUnfreePredicate = pkg: builtins.elem (lib.getName pkg) [ "openbcm-bcm-user" "openbcm-libsdk" ];
  networking.hostName = "as7760";
  # SONiC switch stack (modules/boards/as7760 wires it to the Rust SAI); on at
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
  # eth0 (DPAA FMan) is the management port: ../lab-access.nix.
  # 2026-11-14: the old "ethtool -K eth0 rxhash off" pre-start workaround was
  # removed. Register reads showed that toggle never bypasses KeyGen (BMI
  # fmbm_rfpne stays NIA_ENG_HWK; it only stops skb_set_hash), and RX with
  # hashing on works under full NixOS. See linux-bringup.md before re-adding.
}
