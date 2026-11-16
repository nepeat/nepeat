# NixOS VM test of services.sonic.bfd (software BFD) with the virtual SAI told
# to offer no BFD offload (as sai-openbcm): bfdd/bfdsyncd/staticroutebfd up,
# BfdOrch hands sessions to software (STATE_DB BFD_SOFTWARE_SESSION_TABLE, no
# SAI BFD object), BGP neighbors with BFD over a LACP PortChannel (default VRF)
# and over a port in Vrf-test, "show bfd summary/peer", BGP dropped by BFD
# (< 1.5 s, hold time 90 s) on 100% loss / link down at the peer, a live BFD
# toggle, static routes with BFD (STATIC_ROUTE bfd=true, default VRF and
# Vrf-test) leaving and re-entering APPL_DB with their sessions, the firewall
# rule, BGP_NEIGHBOR local_asn (live and at creation; replace-as/no-prepend AS
# paths), teamd respawn keeping a PortChannel's mac_addr, bfdd/bfdsyncd CPU.
# Links: eth1+eth2 = PortChannel0001 (BGP, default VRF), eth3 = Ethernet7/1
# (BGP, Vrf-test), eth4 = Ethernet8/1 (static route, default VRF), eth5 =
# Ethernet9/1 (static route, Vrf-test). Static and BGP BFD never share a peer
# address: bfdd matches sessions to one peer with an unset local address or
# interface as wildcards, so a second session to the same address can capture
# the first one's packets (README, limits).
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-bfd.nix { inherit pkgs; }'
{ pkgs }:
pkgs.testers.runNixOSTest {
  name = "sonic-bfd";
  nodes.switch = { lib, ... }: {
    imports = [ ./common.nix ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    virtualisation.vlans = lib.mkForce [ 1 2 3 4 5 ];
    networking.interfaces = lib.genAttrs [ "eth1" "eth2" "eth3" "eth4" "eth5" ] (_: {
      ipv4.addresses = lib.mkForce [ ];
      ipv6.addresses = lib.mkForce [ ];
    });
    # Firewall on (nftables), so the BFD/BGP rules of bfd.nix/frr.nix are what lets the sessions up.
    networking.firewall.enable = true;
    networking.nftables.enable = true;
    services.sonic.sai.vs.interfaces = { "Ethernet7/1" = "eth3"; "Ethernet8/1" = "eth4"; "Ethernet9/1" = "eth5"; };
    services.sonic.settings.DEVICE_METADATA.localhost.bgp_asn = "65100";
  };
  nodes.peer = { lib, ... }: {
    imports = [ ./peer.nix ];
    virtualisation.vlans = lib.mkForce [ 1 2 3 4 5 ];
    networking.interfaces = lib.mapAttrs (_: a: {
      ipv4.addresses = lib.mkForce [ { address = a; prefixLength = 24; } ];
      ipv6.addresses = lib.mkForce [ ];
    }) { eth3 = "10.1.0.2"; eth4 = "10.2.0.2"; eth5 = "10.3.0.2"; };
    services.frr = {
      bgpd.enable = true;
      bfdd.enable = true;
      # eth4/eth5: the static routes' BFD sessions (configured here, from
      # staticroutebfd on the switch); bond0/eth3: BGP's (dynamic, from
      # "neighbor ... bfd"). FRR defaults: 300 ms x3.
      config = ''
        ip route 192.0.2.0/24 blackhole
        bfd
         peer 10.2.0.1 local-address 10.2.0.2 interface eth4
         !
         peer 10.3.0.1 local-address 10.3.0.2 interface eth5
         !
        !
        router bgp 65200
         bgp router-id 10.0.0.2
         no bgp ebgp-requires-policy
         neighbor 10.0.0.1 remote-as 65100
         neighbor 10.0.0.1 timers 30 90
         neighbor 10.0.0.1 timers connect 5
         neighbor 10.0.0.1 update-source 10.0.0.2
         neighbor 10.0.0.1 bfd
         neighbor 10.1.0.1 remote-as 65100
         neighbor 10.1.0.1 timers 30 90
         neighbor 10.1.0.1 timers connect 5
         neighbor 10.1.0.1 update-source 10.1.0.2
         neighbor 10.1.0.1 bfd
         address-family ipv4 unicast
          network 192.0.2.0/24
         exit-address-family
      '';
    };
  };
  testScript = builtins.readFile ./vm-bfd-test.py;
}
