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
  # Hard fail (../../switch/openbcm.nix mdioScript.requiredBy): no SONiC
  # without the PHYs. phy-status, the status files and the failure messages
  # name PHYs by cage (extphy.md: bus b holds addresses 4b..4b+3, cages in
  # the order below) and the five verified registers by what they are.
  hardware.openbcm.mdioScript = lib.mkIf config.services.sonic.enable {
    enable = true;
    sdkConfig = config.system.build.sonic.sdkConfig;
    files = [ "/var/lib/evora-phy/evora-100g-bypass.mdio" ];
    phyNames = lib.listToAttrs (lib.concatLists (lib.imap0 (g: base:
      lib.genList (i: lib.nameValuePair "0x${lib.toHexString (base + i)}" "cage${toString (g * 4 + i + 1)}") 4)
      [ 0 36 72 108 272 308 344 380 ]));   # 0x00 0x24 0x48 0x6c 0x110 0x134 0x158 0x17c
    registerNames = { "1.8b00" = "id"; "1.8b01" = "id2"; "1.8215" = "fw"; "1.8245" = "cksum"; "1.8251" = "sides"; };
    hints = {
      script-missing = "the PHY script is private and installed by hand: on the workstation run devices/edge-core-as7772-32x/evora-rehost/install-script.sh, then here: systemctl restart sonic.target";
      script-malformed = "the PHY script is truncated or damaged: install it again (evora-rehost/install-script.sh checks its sha256), then: systemctl restart sonic.target";
      phy-not-answering = "an Evora PHY that reads one constant is hung until a COLD power cycle (lab plug 1, or pull the cord); a reboot or a re-run does not help. It boots by itself afterwards";
      sequence-diverged = "run it again: systemctl restart sonic.target (about 100 s). If it stops at the same line twice, cold power cycle; phy-status --live shows every PHY";
      verify-mismatch = "run it again: systemctl restart sonic.target (about 100 s); a PHY that still differs after a cold power cycle is a hardware or script problem";
      sdk-busy = "systemctl stop sonic.target openbcm-asic-temp (and any bcm.user), then: systemctl restart sonic.target";
      interrupted = "the PHYs may be half-programmed: systemctl restart sonic.target runs the bring-up again (it resets them first)";
    };
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
    }) // {
      # The T2080 die diode (MAX1617 remote channel, ./tpm-sensor.nix; it
      # follows CPU load: devices/edge-core-as7772-32x/tpm-sensor.md).
      cpu_die = { type = "hwmon"; hwmon = { name = "max1617"; }; input = "temp2"; plausible = [ 0 125 ]; critical = 100; };
    } // lib.optionalAttrs (config.hardware.openbcm.mdioScript.enable && config.hardware.openbcm.temperature.enable) {
      # The ASIC cannot be read from power-on until the PHY bring-up has the
      # SDK up (it then publishes the temperature itself), and not at all
      # while a failed bring-up is being looked at with bcm.user: "not
      # online yet" is no fault for 10 min after fand starts; the cpu and
      # board zones run the fans meanwhile (the owner's rule: a cool CPU
      # means a cool ASIC while the ASIC does nothing). After that, and for
      # a reading that was there and went away, it is the usual failsafe.
      # grace: bring-up -> syncd hand-over (syncd's "init all" comes first).
      asic = { startup_grace = "10m"; grace = lib.mkForce "45s"; };
    };
    zones.board.sensors = [ "board_4" "board_5" ];
    # Start-up duty (owner: start lower than fand's default 100 %): 60 % is
    # one CPLD step (raw 153) above the hardware minimum of 53 %, held for the
    # 10 s dwell while the first readings arrive, then the zones' own demand
    # (the floor on a cool box). The board and cpu zones read from the first
    # tick and go up at 10 %/s if anything is warm, and any fault is still
    # failsafe 100 % at once; before fand starts the CPLD runs the fans at
    # full speed by itself.
    startup_duty = 60;
    # CPU die: 32-40 C at the fan minimum here (idle to 8 busy threads), T2080
    # junction limit 105 C. Flat at the minimum to 65 C, 100 % at 90 C,
    # failsafe at 100 C: it only ever acts when the CPU really is hot, or as
    # the stand-in while the ASIC sensor is not online.
    zones.cpu = {
      outputs = [ "fans" ]; sensors = [ "cpu_die" ];
      policy.curve = { points = [ [ 65 53 ] [ 80 75 ] [ 90 100 ] ]; hysteresis = 3; };
    };
  };
}) ];
}
