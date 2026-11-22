# AS7772-32X Broadcom SDK config (config.bcm) for services.sonic.sai.sdkConfig.
# Same ASIC, cage order and port numbering as the AS7760 (../as7760/sdk-config.nix),
# but every ASIC lane ends at a BCM82391 MACsec PHY instead of a cage, so the
# board values differ:
#   lane maps    this board's own (laneMaps below, from the factory 32x100G
#                and 128x25G configs, which agree): 27 of 32 cores differ from
#                the AS7760.
#   polarity     none: the factory configs flip no ASIC lane.
#   TX taps      none: the public AS7712 pre-emphasis is tuned for ASIC-to-cage
#                traces that do not exist here, and the factory sets no ASIC
#                taps (its script tunes the PHY line side instead). SDK defaults.
#   rate_ext_mdio_divisor=53, rate_ext_mdio_dividend=1, mdio_output_delay=10
#                the factory's external MDIO clock (about 8 MHz; SDK default
#                about 2 MHz) for the PHYs' MDIO buses, and the SDK's default
#                hold delay, pinned (see the settings below).
# Everything else is ../as7760/sdk-config.nix (this file only passes it the
# differences): the public AS7712 file's global keys with the AS7760 decisions
# (ccm_dma/ccmdma_intr/mem_cache off, no ALPM, management ports 66 and 100).
# Comparison with the factory files and the decision for each remaining
# difference: devices/edge-core-as7772-32x/sdk-config.md.
# Not covered here: the PHYs themselves (hardware.openbcm.mdioScript in
# ./default.nix initialises them at boot) and the ASIC-side port settings
# (interface, FEC, IPG: the SAI_OPENBCM_PORT_* keys there).
#   import ./sdk-config.nix { inherit openbcm sonic; } { ports = [ ... ]; settings = { }; }
{ openbcm, sonic }:
let
  cages = import ../../switch/sonic/port-maps/accton-as7712-32x.nix;
  # [ tx rx ] lane map of each cage's SerDes core, list index = cage - 1
  # (SDK notation: digit i from the right = the lane wired to logical lane i;
  # 0x3210 is straight). Board layout: ASIC lanes to the PHY's system side.
  laneMaps = [
    [ "0x3210" "0x0123" ] [ "0x3210" "0x0123" ] [ "0x3210" "0x0123" ] [ "0x3210" "0x0123" ]  # cages 1-4
    [ "0x0123" "0x3210" ] [ "0x3210" "0x0123" ] [ "0x3210" "0x0123" ] [ "0x0123" "0x0123" ]  # 5-8
    [ "0x0123" "0x0123" ] [ "0x3210" "0x3210" ] [ "0x0123" "0x0123" ] [ "0x3210" "0x3210" ]  # 9-12
    [ "0x3210" "0x2301" ] [ "0x0123" "0x1203" ] [ "0x3210" "0x2301" ] [ "0x0123" "0x0123" ]  # 13-16
    [ "0x3210" "0x2301" ] [ "0x0123" "0x1032" ] [ "0x3210" "0x2301" ] [ "0x0123" "0x1203" ]  # 17-20
    [ "0x0123" "0x0123" ] [ "0x3210" "0x3210" ] [ "0x0123" "0x0123" ] [ "0x3210" "0x3210" ]  # 21-24
    [ "0x3210" "0x2103" ] [ "0x0123" "0x3210" ] [ "0x0123" "0x3210" ] [ "0x3210" "0x0123" ]  # 25-28
    [ "0x0123" "0x3210" ] [ "0x0123" "0x3210" ] [ "0x0123" "0x3210" ] [ "0x0123" "0x3210" ]  # 29-32
  ];
in
import ../as7760/sdk-config.nix {
  inherit openbcm sonic;
  board = public: {
    name = "as7772-32x-config.bcm";
    base = assert builtins.length laneMaps == builtins.length cages;
      openbcm.configs.tomahawkBase {
        name = "as7772-32x-base.config.bcm";
        globals = public.path;
        cores = builtins.genList (i: let m = builtins.elemAt laneMaps i; in {
          lane = builtins.head (builtins.elemAt cages i);
          tx = builtins.elemAt m 0;
          rx = builtins.elemAt m 1;
        }) (builtins.length cages);
      };
    label = "the AS7772-32X lane maps (nix/modules/boards/as7772/sdk-config.nix) and the global keys of ${public.label}";
    laneKeys = [ ];
    settings = {
      # External MDIO to the Evora PHYs: factory rate (about 8 MHz) and the SDK's
      # default hold delay, pinned. Both are programmed by "init misc" only: after
      # a bare "init soc" the chip runs MDC at about 17.7 MHz with delay 3, which
      # corrupts frames and hangs PHYs until a power cycle (extphy.md, 2026-11-22).
      rate_ext_mdio_divisor = "53";
      rate_ext_mdio_dividend = "1";
      mdio_output_delay = "10";
    };
  };
}
