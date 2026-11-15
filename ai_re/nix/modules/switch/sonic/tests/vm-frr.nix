# NixOS VM test of services.sonic.frr (SONiC's routing stack) with the
# virtual SAI: FRR daemons + fpmsyncd + bgpcfgd up, FRR config rendered from
# CONFIG_DB, an eBGP session to the peer VM (tests/peer.nix + FRR) over a LACP
# PortChannel, learned prefixes in APPL_DB ROUTE_TABLE, ASIC_DB and the
# kernel, a CONFIG_DB static route through bgpcfgd/staticd, "show ip route",
# "config interface ip remove" (incl. SONiC's refusal while a static route
# uses the RIF), withdraw, a zebra restart (bgpcfgd replays CONFIG_DB), crash
# recovery (recovery.nix: teamd respawn, orchagent -> syncd, syncd ->
# orchagent, ASIC-temperature handover) and "config reload" with no saved file.
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-frr.nix { inherit pkgs; }'
{ pkgs }:
pkgs.testers.runNixOSTest {
  name = "sonic-frr";
  nodes.switch = { lib, pkgs, ... }: {
    imports = [ ./common.nix ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    virtualisation.vlans = [ 1 2 ];
    networking.interfaces = lib.genAttrs [ "eth1" "eth2" ] (_: {
      ipv4.addresses = lib.mkForce [ ];
      ipv6.addresses = lib.mkForce [ ];
    });
    networking.firewall.enable = false;
    # Declared in Nix (enforced): the local AS. Neighbors are runtime CONFIG_DB.
    services.sonic.settings.DEVICE_METADATA.localhost.bgp_asn = "65100";
    # Stand-in for openbcm-asic-temp (handover around syncd restarts).
    systemd.services.fake-asic-temp = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
    };
    services.sonic.sai.conflicts = [ "fake-asic-temp.service" ];
    services.sonic.sai.restartOnStop = [ "fake-asic-temp.service" ];
  };
  nodes.peer = { ... }: {
    imports = [ ./peer.nix ];
    services.frr = {
      bgpd.enable = true;
      config = ''
        ip route 192.0.2.0/24 blackhole
        ip route 198.51.100.0/24 blackhole
        router bgp 65200
         bgp router-id 10.0.0.2
         no bgp ebgp-requires-policy
         neighbor 10.0.0.1 remote-as 65100
         neighbor 10.0.0.1 timers 3 9
         neighbor 10.0.0.1 timers connect 5
         address-family ipv4 unicast
          network 192.0.2.0/24
          network 198.51.100.0/24
         exit-address-family
      '';
    };
  };
  testScript = builtins.readFile ./vm-frr-test.py;
}
