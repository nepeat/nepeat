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
    no-such-port-setting = { cfg = { services.sonic.settings.PORT."Ethernet1/9".admin_status = "up"; }; expect = "settings.PORT.\"Ethernet1/9\": no such port"; };
  };
  results = lib.mapAttrs (n: c: let m = failed c.cfg; in {
    inherit (c) expect; messages = m;
    ok = lib.any (lib.hasInfix c.expect) m;
  }) cases;
  good = failed { };
  bad = lib.filterAttrs (_: r: !r.ok) results;
  goodSys = (eval { }).config.system.build.sonic;
  yangBad = (eval { services.sonic.settings.VLAN.Vlan5000.vlanid = "5000"; }).config.system.build.sonic.base;
in pkgs.runCommand "sonic-module-eval-tests" {
  passAsFile = [ "report" ];
  report = builtins.toJSON { inherit results good; };
  yangFailure = pkgs.testers.testBuildFailure yangBad;
  goodBase = goodSys.base;
} ''
  cat $reportPath
  echo
  ${lib.optionalString (good != [ ]) "echo 'good config has failed assertions'; exit 1"}
  ${lib.optionalString (bad != { }) "echo 'cases without the expected message: ${toString (lib.attrNames bad)}'; exit 1"}
  grep -q "fails YANG validation" $yangFailure/testBuildFailure.log
  echo "YANG negative: $(grep -o 'fails YANG validation.*' $yangFailure/testBuildFailure.log | head -c 300)"
  grep -q '"Ethernet1/2"' $goodBase && grep -q '"lanes":"50"' $goodBase
  echo "all ${toString (lib.length (lib.attrNames cases))} negative cases fail as expected; good config passes" | tee $out
''
