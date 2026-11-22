# Build-time tests of the AS7772 config.bcm generator (./sdk-config.nix): pinned
# 32x100G output, this board's lane maps on all 32 cores and none of the
# AS7760's cage-side values (no polarity flips, no TX taps), the external MDIO
# rate, a split cage carries its core's lane map on every subport, an
# impossible layout fails. The Tomahawk rules themselves are tested by
# ../as7760/sdk-config-test.nix. With system = the as7772 nixosSystem, also the
# wiring: turned on, SONiC would use this generator (32x100G) and not the AS7760's.
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:'$PWD'/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; }; in
#     import ./nix/modules/boards/as7772/sdk-config-test.nix { inherit pkgs; system = f.nixosConfigurations.as7772; }'
{ pkgs, system ? null }:
let
  inherit (pkgs) lib;
  args = {
    openbcm = import ../../../packages/openbcm { inherit pkgs; };
    sonic = import ../../../packages/sonic { inherit pkgs; };
  };
  gen = ports: import ./sdk-config.nix args { inherit ports; };
  cages = import ../../switch/sonic/port-maps/accton-as7712-32x.nix;
  portsOf = breakout: (import ../../switch/sonic/ports.nix { inherit lib; } {
    inherit cages breakout;
    defaultSpeed = 100000;
    validSpeeds = { "1" = [ 10000 25000 ]; "2" = [ 50000 ]; "4" = [ 40000 100000 ]; };
  }).ports;
  p = alias: lanes: speed: { inherit alias lanes speed; };
  x4 = cage: speed: lib.imap1 (i: l: p "Ethernet${toString cage}/${toString i}" [ l ] speed) (lib.elemAt cages (cage - 1));

  default = gen (portsOf { });
  # cage 13 (lanes 81-84, logical 84-87): tx straight, rx 0x2301.
  split = gen (portsOf { "13" = x4 13 25000; });
  as7760 = import ../as7760/sdk-config.nix args { ports = portsOf { }; };
  bad = pkgs.testers.testBuildFailure (gen (lib.filter (q: q.alias != "Ethernet13/1") (portsOf { "13" = x4 13 25000; })));
  sys = system.extendModules { modules = [ { services.sonic.enable = lib.mkForce true; } ]; };
in pkgs.runCommand "as7772-sdk-config-tests" ({
  inherit default split as7760 bad;
  base = default.base;
  nativeBuildInputs = [ pkgs.diffutils ];
} // lib.optionalAttrs (system != null) {
    sysConfig = sys.config.system.build.sonic.sdkConfig;
    sysProfile = sys.config.system.build.sonic.saiProfile;
  }) ''
  set -euo pipefail
  # Stability: change this only with a reviewed generator or board-data change.
  # (2026-11-22: header comment wrapped, its second line was 285 characters and the SDK
  #  read the rest as "format error ... on line 3"; the old pin 1c884e6b... differs in comment lines only)
  echo "10cbd0b24549b57bd3bb0c7f3726d8a91ba1f6fe429faf097237174313441d7a  $default" | sha256sum -c
  ! grep -qE '.{255}' $default
  # The base (reference globals + this board's cores; tomahawk.py base).
  echo "c9e8a135d54da69bfdede48df87fb704bfb7942691db52465d24ca19d1ab61ec  $base" | sha256sum -c

  # 32 ports + 2 management, the AS7760's ports and numbering (same cages).
  diff <(grep -E '^portmap_' $default | sort) <(grep -E '^portmap_' $as7760 | sort)
  [ "$(grep -cE '^portmap_' $default)" = 34 ]
  # A lane map pair on every core; none of the AS7760's cage-side values.
  [ "$(grep -cE '^xgxs_tx_lane_map_[0-9]+=0x[0-3]{4}$' $default)" = 32 ]
  [ "$(grep -cE '^xgxs_rx_lane_map_[0-9]+=0x[0-3]{4}$' $default)" = 32 ]
  ! grep -qE '^(phy_xaui_|serdes_)' $default
  ! grep -qE '^(phy_xaui_|serdes_)' $base
  grep -q '^serdes_preemphasis' $as7760 && grep -q '^phy_xaui_' $as7760
  # Spot values (factory 32x100G): cage 1 = logical 50, 13 = 84, 25 = 118, 29 = 1.
  for k in xgxs_tx_lane_map_50=0x3210 xgxs_rx_lane_map_50=0x0123 xgxs_rx_lane_map_84=0x2301 \
           xgxs_rx_lane_map_118=0x2103 xgxs_tx_lane_map_1=0x0123 xgxs_rx_lane_map_1=0x3210 \
           rate_ext_mdio_divisor=53 rate_ext_mdio_dividend=1 mdio_output_delay=10 ccm_dma_enable=0 mem_cache_enable=0; do
    grep -qx $k $default
  done
  ! grep -q '^l3_alpm_enable' $default
  # Only the lane maps differ from the AS7760's global keys and ports.
  diff <(grep -vE '^(#|$|xgxs_|phy_xaui_|serdes_|rate_ext_mdio_|mdio_output_delay=)' $default) <(grep -vE '^(#|$|xgxs_|phy_xaui_|serdes_)' $as7760)

  # Split cage: 4 portmaps, the core's lane maps on every subport.
  for l in 84 85 86 87; do
    grep -qx "portmap_$l=$((l - 3)):25" $split
    grep -qx "xgxs_tx_lane_map_$l=0x3210" $split
    grep -qx "xgxs_rx_lane_map_$l=0x2301" $split
  done
  diff <(grep -vE '_(84|85|86|87)=|^#|^pbmp_|^dport_map_port_|^$' $default) <(grep -vE '_(84|85|86|87)=|^#|^pbmp_|^dport_map_port_|^$' $split)

  grep -q "unused while others are" $bad/testBuildFailure.log
${lib.optionalString (system != null) ''
  # Wiring: the as7772 system with SONiC forced on uses this generator.
  cmp $sysConfig $default
  grep -qx "SAI_INIT_CONFIG_FILE=$sysConfig" $sysProfile
  echo "wiring: $sysProfile -> $sysConfig"
''}
  echo "default $(sha256sum < $default | cut -c1-16): all checks pass" | tee $out
''
