# Build-time tests of the AS7760 config.bcm generator (./sdk-config.nix +
# packages/openbcm/configs/tomahawk.py): pinned output for the 32x100G default
# and the lab layout (stability), a split cage = 4 portmaps with Tomahawk-valid
# logical numbers and per-lane keys split as the SDK reads them, the other
# cages untouched by a split, the other port modes (2x50G, 50+25+25G, 4x10G,
# 40G) build, and layouts a Tomahawk cannot do fail the build. With system =
# the as7760 nixosSystem, also the wiring: with SONiC on, the generated file is
# the lab layout and sai.profile's SAI_INIT_CONFIG_FILE.
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:'$PWD'/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; }; in
#     import ./nix/modules/boards/as7760/sdk-config-test.nix { inherit pkgs; system = f.nixosConfigurations.as7760; }'
{ pkgs, system ? null }:
let
  inherit (pkgs) lib;
  gen = ports: import ./sdk-config.nix {
    openbcm = import ../../../packages/openbcm { inherit pkgs; };
    sonic = import ../../../packages/sonic { inherit pkgs; };
  } { inherit ports; };
  cages = import ../../switch/sonic/port-maps/accton-as7712-32x.nix;
  portsOf = breakout: (import ../../switch/sonic/ports.nix { inherit lib; } {
    inherit cages breakout;
    defaultSpeed = 100000;
    validSpeeds = { "1" = [ 10000 25000 ]; "2" = [ 50000 ]; "4" = [ 40000 100000 ]; };
  }).ports;
  p = alias: lanes: speed: { inherit alias lanes speed; };
  x4 = cage: speed: lib.imap1 (i: l: p "Ethernet${toString cage}/${toString i}" [ l ] speed) (lib.elemAt cages (cage - 1));
  # Same layout as configs/as7760 (cages 1 and 32 split 4x25G).
  lab = { "1" = x4 1 25000; "32" = x4 32 25000; };

  default = gen (portsOf { });
  labCfg = gen (portsOf lab);
  # cage 13 (lanes 81-84): the public file flips its polarity (rx 0xf, tx 0x6)
  # and swaps its lanes (0x1032): split, each subport carries its lane's part.
  split = gen (portsOf { "13" = x4 13 25000; });
  modes = gen (portsOf {
    "2" = [ (p "Ethernet2/1" [ 53 54 ] 50000) (p "Ethernet2/2" [ 55 56 ] 50000) ];
    "3" = [ (p "Ethernet3/1" [ 57 58 ] 50000) (p "Ethernet3/2" [ 59 ] 25000) (p "Ethernet3/3" [ 60 ] 25000) ];
    "4" = [ (p "Ethernet4/1" [ 61 ] 25000) (p "Ethernet4/2" [ 62 ] 25000) (p "Ethernet4/3" [ 63 64 ] 50000) ];
    "5" = x4 5 10000;
    "6" = [ (p "Ethernet6/1" [ 69 70 71 72 ] 40000) ];
  });
  bad = {
    two_lanes_at_lane_1 = { ports = portsOf { "1" = [ (p "Ethernet1/1" [ 49 ] 25000) (p "Ethernet1/2" [ 50 51 ] 50000) (p "Ethernet1/3" [ 52 ] 25000) ]; };
                            expect = "must start at lane 0 or 2"; };
    mixed_10g_25g = { ports = portsOf { "1" = [ (p "Ethernet1/1" [ 49 ] 10000) (p "Ethernet1/2" [ 50 ] 25000) (p "Ethernet1/3" [ 51 ] 25000) (p "Ethernet1/4" [ 52 ] 25000) ]; };
                      expect = "one VCO per core"; };
    lane_0_unused = { ports = lib.filter (q: q.alias != "Ethernet1/1") (portsOf lab); expect = "unused while others are"; };
  };
  sys = system.extendModules { modules = [ { services.sonic.enable = lib.mkForce true; } ]; };
in pkgs.runCommand "as7760-sdk-config-tests" ({
  inherit default labCfg split modes;
  nativeBuildInputs = [ pkgs.diffutils ];
} // lib.mapAttrs' (n: c: lib.nameValuePair "fail_${n}" (pkgs.testers.testBuildFailure (gen c.ports))) bad
  // lib.optionalAttrs (system != null) {
    sysConfig = sys.config.system.build.sonic.sdkConfig;
    sysProfile = sys.config.system.build.sonic.saiProfile;
    sysInit = sys.config.services.sonic.sai.initConfigFile;
  }) ''
  set -euo pipefail
  # Stability: change these only with a reviewed generator change.
  echo "c6342e9856f481c190566455f4930ed5db0a1d76694c4eb18eca1186fa405948  $default" | sha256sum -c
  echo "c1ed67378bc5b0897c0efc3efe6ac67b35cbcf7d68889daea1858a3dddc6bcd8  $labCfg" | sha256sum -c

  pm() { grep -E '^portmap_[0-9]+=' "$1" | sort; }
  # Every portmap: logical number in its pipe's range, never a fixed port.
  for f in $default $labCfg $split $modes; do
    pm $f | awk -F'[_=:]' '{ l = $2; ph = $3
      if (ph <= 128) { pipe = int((ph - 1) / 32); lo = (pipe == 0) ? 1 : (pipe == 1) ? 34 : (pipe == 2) ? 68 : 102
                       hi = (pipe == 0) ? 32 : (pipe == 1) ? 66 : (pipe == 2) ? 100 : 133 }
      else if (ph == 129) { lo = 34; hi = 66 } else if (ph == 131) { lo = 68; hi = 100 } else { lo = 1; hi = 0 }
      if (l < lo || l > hi || l == 33 || l == 67 || l == 101) { print "bad: " $0; bad = 1 } }
      END { exit bad }'
  done
  [ "$(pm $default | wc -l)" = 34 ] && [ "$(pm $labCfg | wc -l)" = 40 ]

  # Lab: cage 1 (lanes 49-52, pipe 1) and cage 32 (13-16, pipe 0) -> 4 portmaps each.
  printf '%s\n' portmap_50=49:25 portmap_51=50:25 portmap_52=51:25 portmap_53=52:25 \
    portmap_13=13:25 portmap_14=14:25 portmap_15=15:25 portmap_16=16:25 | sort > want
  pm $labCfg | grep -E '=(49|50|51|52|13|14|15|16):' > got
  diff want got
  # The other 30 cages keep their portmaps (no renumbering).
  diff <(pm $default | grep -vE '=(49|13):') <(pm $labCfg | grep -vE '=(49|50|51|52|13|14|15|16):')

  # Split core: lane map on every subport, polarity split per lane (1-lane
  # ports: bit 0 = their lane), pre-emphasis lane0 of each subport.
  for l in 84 85 86 87; do
    grep -qx "xgxs_tx_lane_map_$l=0x1032" $split
    grep -qx "phy_xaui_rx_polarity_flip_$l=0x1" $split
    grep -q "^serdes_preemphasis_lane0_$l=" $split
  done
  [ "$(grep -E '^phy_xaui_tx_polarity_flip_(84|85|86|87)=' $split | tr '\n' ' ')" = \
    "phy_xaui_tx_polarity_flip_84=0x0 phy_xaui_tx_polarity_flip_85=0x1 phy_xaui_tx_polarity_flip_86=0x1 phy_xaui_tx_polarity_flip_87=0x0 " ]
  ! grep -q "^serdes_preemphasis_lane1_8[4-7]=" $split
  # Other modes: 2x50G, 50+25+25G, 25+25+50G, 4x10G (no 25G taps), 40G.
  for k in portmap_54=53:50 portmap_56=55:50 portmap_58=57:50 portmap_60=59:25 portmap_61=60:25 \
           portmap_64=63:50 portmap_68=65:10 portmap_72=69:40; do
    grep -qx $k $modes
  done
  ! grep -qE '^serdes_(preemphasis|driver_current)_lane[0-3]_(68|69|70|71|72)=' $modes
  ! grep -q '^serdes_driver_current' $default

  # Impossible layouts fail the build with the reason.
${lib.concatStrings (lib.mapAttrsToList (n: c: ''
  grep -q ${lib.escapeShellArg c.expect} $fail_${n}/testBuildFailure.log
  echo "fails as expected (${n}): $(grep -o 'config.bcm: .*' $fail_${n}/testBuildFailure.log | head -c 200)"
'') bad)}
${lib.optionalString (system != null) ''
  # Wiring: the as7760 system (SONiC on) generates the lab layout and points
  # SAI_INIT_CONFIG_FILE at it.
  cmp $sysConfig $labCfg
  [ "$sysInit" = "$sysConfig" ]
  grep -qx "SAI_INIT_CONFIG_FILE=$sysConfig" $sysProfile
  echo "wiring: $sysProfile -> $sysConfig"
''}
  echo "default $(sha256sum < $default | cut -c1-16) lab $(sha256sum < $labCfg | cut -c1-16): all checks pass" | tee $out
''
