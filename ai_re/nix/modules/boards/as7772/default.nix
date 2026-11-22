# Accton/Edgecore AS7772-32X: the AS7760-32X board (same T2080 control plane,
# fan CPLD, sensors, kernel) with MACsec PHYs in front of the ASIC, a TPM and
# an M.2 SATA SSD. Everything comes from ../as7760; this module holds the
# deltas. Install: ../as7760/sd-install.nix (the same image layout, written to
# the SSD; upstream U-Boot's standard boot reads extlinux.conf from p1).
{ config, lib, pkgs, aiReRust ? null, ... }:
let
  sonic = import ../../../packages/sonic { inherit pkgs; };
  openbcm = import ../../../packages/openbcm { inherit pkgs; };
  boards = ../../../packages/linux-qoriq-t2080/boards;
  dts = pkgs.buildPackages.runCommand "as7772-vendor.dts" { } ''
    cat ${builtins.path { path = boards + "/as7760/as7760-vendor.dts"; name = "as7760-vendor.dts"; }} \
        ${builtins.path { path = boards + "/as7772/as7772-delta.dtsi"; name = "as7772-delta.dtsi"; }} > $out
  '';
in {
  imports = [ ../as7760 ./tpm-sensor.nix ];
  # mkMerge, not "//": the fand block below also defines services.*, and a
  # shallow update would drop every services.sonic line of the first block
  # (it did until 2026-11-22: root-flake builds got the AS7760 SDK config).
  config = lib.mkMerge [ {
  hardware.as7760.deviceTreeSource = dts;
  # SDK modules and the die temperature reader (attach only) come from
  # ../as7760. The ASIC sits behind BCM82391 "Evora" MACsec PHYs here: the
  # cage order is the AS7760's (services.sonic.cages from ../as7760), the SDK
  # config is this board's own (./sdk-config.nix: other lane maps, no polarity
  # flips, no ASIC TX taps). SONiC is enabled by the config
  # (configs/as7772); devices/edge-core-as7772-32x/sonic.md.
  # The PHYs are brought up once per boot by a recorded MDIO script
  # (../../switch/openbcm.nix mdioScript). The script holds the vendor
  # firmware, so it is not in this repository or the store: it is made on the
  # workstation and copied to /var/lib/evora-phy by hand
  # (devices/edge-core-as7772-32x/sonic.md).
  # On with SONiC: the SDK config it needs (MDIO clock, hold delay) is the
  # one generated from the SONiC ports.
  hardware.openbcm.mdioScript = lib.mkIf config.services.sonic.enable {
    enable = true;
    sdkConfig = config.system.build.sonic.sdkConfig;
    files = [ "/var/lib/evora-phy/evora-100g-bypass.mdio" ];
  };
  # ASIC side of every front-panel port (sai-openbcm port quirks): the lanes
  # end at the PHY's system side, which is plain CAUI without autoneg; FEC is
  # done by the PHY on the cage side, so the SONiC FEC setting is kept but not
  # programmed into the ASIC; average IPG 16 bytes as the factory's bypass
  # setup (mac_ipg_16.soc; 48 with MACsec on).
  services.sonic.sai.profile = {
    SAI_OPENBCM_PORT_INTERFACE = "caui";
    SAI_OPENBCM_PORT_AUTONEG = "off";
    SAI_OPENBCM_PORT_FEC = "none";
    SAI_OPENBCM_PORT_IFG = "128";
  };
  # ../as7760 would load the AS7712 LED program; this board's LED chain is
  # another one (factory ES7632BTx.hex, not converted): none, LEDs stay dark.
  hardware.as7760.ledProgram = null;
  # pmon: the AS7760 sonic_platform with this board's sensors (six LM75s and
  # the MAX1617: local + T2080 die diode).
  services.sonic.pmon.platformPackage = pkgs.callPackage ../as7760/sonic-platform-package.nix {
    sonic = config.services.sonic.packages;
    pname = "sonic-platform-accton-as7772-32x";
    unitTest = ./sonic_platform_test.py;
    thermals = [
      [ "board_1" "lm75" 72 1 45 50 ] [ "board_2" "lm75" 73 1 45 50 ] [ "board_3" "lm75" 74 1 45 50 ]
      [ "cpu_board" "lm75" 75 1 45 50 ] [ "board_4" "lm75" 76 1 45 50 ] [ "board_5" "lm75" 77 1 45 50 ]
      [ "cpu_area" "max1617" 24 1 70 80 ] [ "cpu_die" "max1617" 24 2 95 105 ]
    ];
  };
  services.sonic.platform = "powerpc64-accton_as7772_32x-r0";
  services.sonic.hwsku = "Accton-AS7772-32X";
  services.sonic.sai.sdkConfig.generator = lib.mkForce (import ./sdk-config.nix { inherit openbcm sonic; });
  # Two more board LM75s than the AS7760 (diag I2C map: LM75_4 0x4c, LM75_5 0x4d).
  environment.etc."sensors.d/as7772.conf".text = ''
    chip "lm75-i2c-*-4c"
      label temp1 "board_4"
    chip "lm75-i2c-*-4d"
      label temp1 "board_5"
  '';
} (lib.optionalAttrs (aiReRust != null) {
  # fand (services.fand exists only with the monorepo's rust flake): the two
  # extra sensors join the board zone of ../../thermal/as7760-fan.nix.
  services.fand.settings = lib.mkIf (config.hardware.as7760Fan.controller == "fand") {
    sensors = lib.genAttrs' [ [ "board_4" "4c" ] [ "board_5" "4d" ] ] (p: lib.nameValuePair (builtins.elemAt p 0) {
      type = "hwmon"; plausible = [ (-10) 110 ];
      hwmon = { name = "lm75"; of_node = "pca9548@76/i2c@1/temperature-sensor@${builtins.elemAt p 1}"; };
    });
    zones.board.sensors = [ "board_4" "board_5" ];
  };
}) ];
}
