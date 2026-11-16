# NixOS VM test of services.sonic.metrics (virtual SAI): the sonic-metrics
# textfile producer against a live stack, read back through the node
# exporter. Switch: PortChannel0001 (Ethernet5/1 + Ethernet6/1, LACP) to the
# peer with eBGP + BFD in the default VRF, Ethernet7/1 in Vrf-test
# (eBGP to the peer, which bgpmon does not report), LLDP, pmon on the fake
# AS7760 board (two modules), an ACL rule, CoPP, a runtime conflict, a killed
# unit; then redis stopped (sonic_up 0, file still published).
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-metrics.nix { inherit pkgs; }'
{ pkgs }:
let
  sonic = import ../../../../packages/sonic { inherit pkgs; };
  platformPackage = pkgs.callPackage ../../../boards/as7760/sonic-platform-package.nix { inherit sonic; };
  fakeRoot = "/var/lib/as7760-fake";
in
pkgs.testers.runNixOSTest {
  name = "sonic-metrics";
  nodes.switch = { lib, pkgs, ... }: {
    imports = [ ./common.nix ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    virtualisation.vlans = lib.mkForce [ 1 2 3 ];
    networking.interfaces = lib.genAttrs [ "eth1" "eth2" "eth3" ] (_: {
      ipv4.addresses = lib.mkForce [ ];
      ipv6.addresses = lib.mkForce [ ];
    });
    networking.firewall.enable = false;
    services.sonic.sai.vs.interfaces."Ethernet7/1" = "eth3";
    services.sonic.settings.DEVICE_METADATA.localhost.bgp_asn = "65100";
    services.sonic.pmon = {
      enable = true;
      inherit platformPackage;
      environment.AS7760_PLATFORM_ROOT = fakeRoot;
    };
    services.sonic.lldp.enable = true;
    monitoring.nodeExporter.enable = true;   # turns services.sonic.metrics on
    systemd.services = {
      fake-as7760 = {
        description = "Fake AS7760 hardware for sonic_platform";
        wantedBy = [ "sonic.target" ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
        script = "rm -rf ${fakeRoot}; ${pkgs.python3}/bin/python3 -I ${../../../boards/as7760/fake_board.py} ${fakeRoot}";
      };
    } // lib.genAttrs (map (d: "sonic-${d}") [ "xcvrd" "thermalctld" "psud" "syseepromd" ]) (_: {
      requires = [ "fake-as7760.service" ];
      after = [ "fake-as7760.service" ];
    });
    environment.systemPackages = [ pkgs.prometheus.cli pkgs.curl ];
  };
  nodes.peer = { lib, ... }: {
    imports = [ ./peer.nix ];
    virtualisation.vlans = lib.mkForce [ 1 2 3 ];
    networking.interfaces.eth3 = {
      ipv4.addresses = lib.mkForce [ { address = "10.1.0.2"; prefixLength = 24; } ];
      ipv6.addresses = lib.mkForce [ ];
    };
    services.lldpd.enable = true;
    services.frr = {
      bgpd.enable = true;
      bfdd.enable = true;
      config = ''
        router bgp 65200
         bgp router-id 10.0.0.2
         no bgp ebgp-requires-policy
         neighbor 10.0.0.1 remote-as 65100
         neighbor 10.0.0.1 timers connect 5
         neighbor 10.0.0.1 bfd
         neighbor 10.1.0.1 remote-as 65100
         neighbor 10.1.0.1 timers connect 5
         address-family ipv4 unicast
          network 192.0.2.0/24
         exit-address-family
        !
        ip route 192.0.2.0/24 blackhole
      '';
    };
  };
  testScript = builtins.readFile ./vm-metrics-test.py;
}
