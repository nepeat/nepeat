# Build-time tests of the CS8210 IFCS config generator (./sdk-config.nix +
# packages/sai-teralynx/configs/teralynx7.py): pinned output for 32x400 (the
# sku line masked: its store path follows nixpkgs), devport numbering of
# breakouts (subports consecutive, later cages shifted), the lab layout (cage 2
# 4x10G on lanes 0-3, which services.sonic's ports.nix cannot express yet, so
# its ports are given directly), and layouts a Teralynx 7 group does not run
# fail the build. With system = a nixosSystem importing ./default.nix with
# services.sonic enabled, also the wiring (sdkConfig, sai.profile).
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:'$PWD'/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; }; in
#     import ./nix/modules/boards/cs8210/sdk-config-test.nix { inherit pkgs; }'
{ pkgs, system ? null }:
let
  inherit (pkgs) lib;
  sonic = import ../../../packages/sonic { inherit pkgs; };
  teralynx = import ../../../packages/sai-teralynx/configs { inherit pkgs; };
  gen = media: ports: import ./sdk-config.nix { inherit pkgs sonic media; } { inherit ports; };
  withSettings = import ./sdk-config.nix { inherit pkgs sonic; } {
    ports = portsOf { };
    settings = { "netdev.auto_create" = "no"; "netdev.multi_interface" = "yes"; mbist_on_init = null; log_level = "debug"; };
  };
  cages = import ../../switch/sonic/port-maps/celestica-cs8210-32x.nix;
  portsOf = breakout: (import ../../switch/sonic/ports.nix { inherit lib; } {
    inherit cages breakout;
    defaultSpeed = 400000;
    inherit (teralynx) validSpeeds;
  }).ports;
  # n ports of k lanes at speed on cage c, from lane 0 (upper lanes unused).
  split = c: n: k: speed: map (i: {
    name = "Ethernet${toString (8 * (c - 1) + i * k)}"; alias = "Ethernet${toString c}/${toString (i + 1)}";
    lanes = lib.sublist (i * k) k (lib.elemAt cages (c - 1)); inherit speed;
  }) (lib.range 0 (n - 1));
  # Ports of all cages, cages in "over" replaced (no ports.nix: unused lanes allowed).
  direct = over: lib.concatLists (lib.imap1 (c: _: over.${toString c} or (split c 1 8 400000)) cages);

  default = gen { } (portsOf { });
  breakout = gen { } (portsOf { "1" = split 1 4 2 100000; "17" = split 17 2 4 200000; "18" = split 18 8 1 50000; });
  lab = gen { } (direct { "2" = split 2 4 1 10000; });
  dac = gen { "17" = "copper"; } (portsOf { });
  bad = {
    mixed_speeds = { ports = direct { "3" = [ (lib.elemAt (split 3 2 4 200000) 0) (lib.elemAt (split 3 2 4 100000) 1) ]; }; expect = "mix lane counts or speeds"; };
    three_of_four = { ports = direct { "3" = lib.take 3 (split 3 4 2 100000); }; expect = "is not a supported mode"; };
    upper_half = { ports = direct { "3" = [ ((lib.elemAt (split 3 2 4 100000) 1) // { alias = "Ethernet3/1"; }) ]; }; expect = "the mode needs lane 0"; };
    three_lanes = { ports = direct { "3" = [ { name = "Ethernet16"; alias = "Ethernet3/1"; lanes = lib.take 3 (lib.elemAt cages 2); speed = 100000; } ]; }; expect = "is no Teralynx 7 lane rate"; };
    two_cages = { ports = direct { "3" = [ { name = "Ethernet16"; alias = "Ethernet3/1"; lanes = [ (lib.last (lib.elemAt cages 1)) ] ++ lib.take 1 (lib.elemAt cages 2); speed = 50000; } ]; }; expect = "span serdes groups"; };
  };
  sys = system.extendModules { modules = [ { services.sonic.enable = lib.mkForce true; } ]; };
in pkgs.runCommand "cs8210-sdk-config-tests" ({
  inherit default breakout lab dac withSettings;
  defaultSide = default.sidecars; breakoutSide = breakout.sidecars; labSide = lab.sidecars; dacSide = dac.sidecars;
  sku = "${sonic.sources.buildimage}/device/celestica/x86_64-cel_midstone-r0/Midstone-200i_32x400/innovium.77700_A";
  nativeBuildInputs = [ pkgs.diffutils ];
} // lib.mapAttrs' (n: c: lib.nameValuePair "fail_${n}" (pkgs.testers.testBuildFailure (gen { } c.ports))) bad
  // lib.optionalAttrs (system != null) {
    sysConfig = sys.config.system.build.sonic.sdkConfig;
    sysProfile = sys.config.system.build.sonic.saiProfile;
  }) ''
  set -euo pipefail
  # Stability: change these only with a reviewed generator change.
  sed 's|^    sku: .*|    sku: SKU|' $default | sha256sum | grep -q '^511e412b8638b1e20d038045bdf7d16b970d0a81cd8259b731e704b29dc2195f '
  (cd $defaultSide && sha256sum port-map init.cmd link-training.cmd layout.json) | sha256sum | grep -q '^bad010ecea35c234e62bad246f57c80675cc03f52e1740a7e19992261222b0c2 '
  # The sku file is the public one, copied.
  cmp "$(sed -n 's|^    sku: "\(.*\)"$|\1|p' $default)" $sku

  # 32x400: devport n = cage n, 8 lanes of its serdes group, KP FEC, ipg 48 on 1-16.
  [ "$(grep -c '^    type: "eth"' $default)" = 32 ]
  [ "$(grep -vc '^#' $defaultSide/port-map)" = 32 ]
  grep -qx '1 209,210,211,212,213,214,215,216 400000 rs' $defaultSide/port-map
  grep -qx '32 33,34,35,36,37,38,39,40 400000 rs' $defaultSide/port-map
  [ "$(grep -c '^    ipg: "48"$' $default)" = 16 ]
  [ "$(tail -1 $defaultSide/init.cmd)" = "ifcs set devport 129 rx_eq_fine_tune 1" ]
  [ "$(wc -l < $defaultSide/init.cmd)" = 33 ]
  [ "$(cut -d' ' -f4 $defaultSide/link-training.cmd | tr '\n' ,)" = "1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16," ]
  grep -q '^  - id: "129"$' $default && grep -q '^    type: "mgmt 0"$' $default
  grep -qx '    traffic_monitor_cpu_queue: "2"' $default   # sFlow samples on SONiC's sflow queue

  # Breakout: cage 1 4x100G (devports 1-4, 2 lanes each), cage 2 -> devport 5,
  # cage 17 2x200G (20, 21), cage 18 8x50G (22-29), cage 32 -> 43.
  [ "$(grep -vc '^#' $breakoutSide/port-map)" = 43 ]
  for l in '1 209,210 100000 rs' '4 215,216 100000 rs' '5 217,218,219,220,221,222,223,224 400000 rs' \
           '20 121,122,123,124 200000 rs' '21 125,126,127,128 200000 rs' '22 41 50000 rs' '29 48 50000 rs' \
           '43 33,34,35,36,37,38,39,40 400000 rs'; do
    grep -qx "$l" $breakoutSide/port-map
  done
  # Host-side 4x100G has link training; line-side 2x200G optics none.
  grep -qx 'ifcs set devport 4 link_training 1' $breakoutSide/link-training.cmd
  ! grep -qE 'devport (20|21) link_training' $breakoutSide/link-training.cmd

  # Lab: cage 2 4x10G = devports 2-5 on lanes 0-3 of group 27, no FEC; cage 3 -> 6.
  for l in '2 217 10000 none' '5 220 10000 none' '6 193,194,195,196,197,198,199,200 400000 rs'; do
    grep -qx "$l" $labSide/port-map
  done
  ! grep -qE 'devport [2-5] link_training' $labSide/link-training.cmd
  # The other cages keep their devports as in the default, shifted by 3 after cage 2.
  diff <(grep -v '^#' $defaultSide/port-map | sed -n '3,$p' | awk '{print $1 + 3, $2, $3, $4}') \
       <(grep -v '^#' $labSide/port-map | sed -n '6,$p')

  # Cage 17 as copper (DAC): link training on devport 17, still KP FEC.
  grep -qx 'ifcs set devport 17 link_training 1' $dacSide/link-training.cmd
  grep -qx '17 121,122,123,124,125,126,127,128 400000 rs' $dacSide/port-map

  # sdkConfig.settings: node options set/removed, "netdev.<k>", log_level.
  grep -A2 '^    netdev:$' $withSettings | tr -d '\n' | grep -q '    - auto_create: "no"      multi_interface: "yes"'
  ! grep -q mbist_on_init $withSettings
  grep -qx '    log_level: "debug"' $withSettings

  # Impossible layouts fail the build with the reason.
${lib.concatStrings (lib.mapAttrsToList (n: c: ''
  grep -q ${lib.escapeShellArg c.expect} $fail_${n}/testBuildFailure.log
  echo "fails as expected (${n}): $(grep -o 'teralynx7: .*' $fail_${n}/testBuildFailure.log | head -c 200)"
'') bad)}
${lib.optionalString (system != null) ''
  # Wiring: the system's sdkConfig is the 32x400 default; its profile points at it and the sidecars.
  # (a cross system has its own sku store path: compare with it masked)
  cmp <(sed 's|^    sku: .*|    sku: SKU|' $sysConfig) <(sed 's|^    sku: .*|    sku: SKU|' $default)
  grep -qx "SAI_INIT_CONFIG_FILE=$sysConfig" $sysProfile
  grep -q '^SAI_TERALYNX_PORT_MAP=/nix/store/.*-sidecars/port-map$' $sysProfile
  grep -q '^SAI_TERALYNX_INIT_CMDS=/nix/store/.*-sidecars/init.cmd:/run/cs8210-sai/preemphasis.cmd:/nix/store/.*-sidecars/link-training.cmd$' $sysProfile
  echo "wiring: $sysProfile -> $sysConfig"
''}
  echo "default $(sha256sum < $default | cut -c1-16): all checks pass" | tee $out
''
