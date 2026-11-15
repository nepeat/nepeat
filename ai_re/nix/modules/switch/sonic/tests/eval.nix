# Eval-time tests of the breakout rules: each bad override must fail with
# its assertion message; the good one (tests/common.nix) must pass. Also a
# build-failure test of the YANG check (VLAN id out of range).
#   nix build --impure --expr 'import <ai_re>/nix/modules/switch/sonic/tests/eval.nix { pkgs = ...; }'
{ pkgs }:
let
  inherit (pkgs) lib;
  eval = extra: pkgs.nixos {
    imports = [ ./common.nix extra ];
    boot.loader.grub.enable = false;
    fileSystems."/" = { device = "none"; fsType = "tmpfs"; };
    system.stateVersion = "26.05";
  };
  failed = extra: map (a: a.message) (lib.filter (a: !a.assertion) (eval extra).config.assertions);
  cage1 = ports: { services.sonic.breakout."1" = lib.mkForce ports; };
  p = alias: lanes: speed: { inherit alias lanes speed; };
  cases = {
    lane-twice = { cfg = cage1 [ (p "Ethernet1/1" [ 49 50 ] 50000) (p "Ethernet1/2" [ 50 51 52 ] 40000) ]; expect = "lanes used more than once: 50"; };
    lane-left-over = { cfg = cage1 [ (p "Ethernet1/1" [ 49 50 ] 50000) ]; expect = "lanes 51,52 of cage 1 are not assigned"; };
    foreign-lane = { cfg = cage1 [ (p "Ethernet1/1" [ 49 50 ] 50000) (p "Ethernet1/2" [ 51 53 ] 50000) ]; expect = "lanes 53 are not lanes of cage 1"; };
    bad-speed = { cfg = cage1 [ (p "Ethernet1/1" [ 49 ] 100000) (p "Ethernet1/2" [ 50 ] 25000) (p "Ethernet1/3" [ 51 ] 25000) (p "Ethernet1/4" [ 52 ] 25000) ]; expect = "speed 100000 is not valid for 1 lane(s)"; };
    wrong-cage-name = { cfg = cage1 [ (p "Ethernet2/1" [ 49 50 ] 50000) (p "Ethernet1/2" [ 51 52 ] 50000) ]; expect = "name must be Ethernet1/<1..4>"; };
    subport-5 = { cfg = cage1 [ (p "Ethernet1/5" [ 49 50 ] 50000) (p "Ethernet1/2" [ 51 52 ] 50000) ]; expect = "name must be Ethernet1/<1..4>"; };
    name-twice = { cfg = cage1 [ (p "Ethernet1/1" [ 49 50 ] 50000) (p "Ethernet1/1" [ 51 52 ] 50000) ]; expect = "port names used twice: Ethernet1/1"; };
    empty = { cfg = cage1 [ ]; expect = "an override must list every resulting port"; };
    no-such-cage = { cfg = { services.sonic.breakout."33" = [ (p "Ethernet33/1" [ 1 ] 25000) ]; }; expect = "no such cage"; };
    subport-order = { cfg = cage1 [ (p "Ethernet1/1" [ 51 52 ] 50000) (p "Ethernet1/2" [ 49 50 ] 50000) ]; expect = "subports must follow the lane order"; };
    no-such-port-setting = { cfg = { services.sonic.settings.PORT."Ethernet1/9".admin_status = "up"; }; expect = "settings.PORT.\"Ethernet1/9\": no such port"; };
  };
  results = lib.mapAttrs (n: c: let m = failed c.cfg; in {
    inherit (c) expect; messages = m;
    ok = lib.any (lib.hasInfix c.expect) m;
  }) cases;
  good = failed { };
  bad = lib.filterAttrs (_: r: !r.ok) results;
  goodSys = (eval { }).config.system.build.sonic;
  # Names from the first lane (SONiC's rule): 2x50G cage 1 -> Ethernet0, Ethernet2.
  twoBy50 = eval (cage1 [ (p "Ethernet1/1" [ 49 50 ] 50000) (p "Ethernet1/2" [ 51 52 ] 50000) ]);
  names = sys: map (q: "${q.alias}=${q.name}") (lib.take 6 sys.config.system.build.sonic.ports);
  namesGood = names (eval { });
  names2x50 = names twoBy50;
  namesOk = names2x50 == [ "Ethernet1/1=Ethernet0" "Ethernet1/2=Ethernet2" "Ethernet2/1=Ethernet4" "Ethernet3/1=Ethernet8" "Ethernet4/1=Ethernet12" "Ethernet5/1=Ethernet16" ]
    && namesGood == [ "Ethernet1/1=Ethernet0" "Ethernet1/2=Ethernet1" "Ethernet1/3=Ethernet2" "Ethernet1/4=Ethernet3" "Ethernet2/1=Ethernet4" "Ethernet3/1=Ethernet8" ]
    && failed (cage1 [ (p "Ethernet1/1" [ 49 50 ] 50000) (p "Ethernet1/2" [ 51 52 ] 50000) ]) == [ ];
  yangBad = (eval { services.sonic.settings.VLAN.Vlan5000.vlanid = "5000"; }).config.system.build.sonic.base;
  # SDK config plumbing (generic): the board's generator gets the ports and
  # settings, its file becomes SAI_INIT_CONFIG_FILE; none without a generator.
  sdk = (eval {
    services.sonic.sai.sdkConfig.generator = { ports, settings }:
      pkgs.writeText "sdk-config-probe.json" (builtins.toJSON { inherit ports settings; });
    services.sonic.sai.sdkConfig.settings.probe_key = "1";
  }).config;
  sdkOk = sdk.services.sonic.sai.initConfigFile == "${sdk.system.build.sonic.sdkConfig}"
    && (eval { }).config.services.sonic.sai.initConfigFile == null;
in pkgs.runCommand "sonic-module-eval-tests" {
  passAsFile = [ "report" ];
  report = builtins.toJSON { inherit results good namesGood names2x50; };
  yangFailure = pkgs.testers.testBuildFailure yangBad;
  goodBase = goodSys.base;
  goodProfile = goodSys.saiProfile;
  sdkProbe = sdk.system.build.sonic.sdkConfig;
  sdkProfile = sdk.system.build.sonic.saiProfile;
} ''
  cat $reportPath
  echo
  ${lib.optionalString (good != [ ]) "echo 'good config has failed assertions'; exit 1"}
  ${lib.optionalString (!namesOk) "echo 'port names not generated from the first lane'; exit 1"}
  ${lib.optionalString (!sdkOk) "echo 'sai.initConfigFile is not the generated SDK config'; exit 1"}
  grep -qx "SAI_INIT_CONFIG_FILE=$sdkProbe" $sdkProfile
  grep -q '"alias":"Ethernet1/2","lanes":\[50\],"name":"Ethernet1","speed":25000' $sdkProbe
  grep -q '"probe_key":"1"' $sdkProbe
  ! grep -q SAI_INIT_CONFIG_FILE $goodProfile
  echo "SDK config: generator gets the ports and settings, its file is SAI_INIT_CONFIG_FILE"
  echo "names 4x25G: ${toString namesGood}"
  echo "names 2x50G: ${toString names2x50}"
  ${lib.optionalString (bad != { }) "echo 'cases without the expected message: ${toString (lib.attrNames bad)}'; exit 1"}
  grep -q "fails YANG validation" $yangFailure/testBuildFailure.log
  echo "YANG negative: $(grep -o 'fails YANG validation.*' $yangFailure/testBuildFailure.log | head -c 300)"
  grep -q '"Ethernet1/2"' $goodBase && grep -q '"lanes":"50"' $goodBase
  echo "all ${toString (lib.length (lib.attrNames cases))} negative cases fail as expected; good config passes" | tee $out
''
