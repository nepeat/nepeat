# bfdd memory growth check (sonic.routing.frr's bfdd), as the AS7760 soak:
# node "dut" runs zebra + mgmtd + bfdd from sonic.routing.frr. Phase "vrf":
# two single-hop sessions over a veth pair between the default VRF and
# Vrf-test on the same box (the switch's lab-cable loop). Phase "novrf": the
# same two sessions moved to two links to node "peer" (nixpkgs FRR), default
# VRF only. 300 ms x3. RSS of bfdd sampled every 30 s per phase.
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-bfdd-leak.nix { inherit pkgs; }'
#   args: minutes (per phase); frr (default: sonic.routing.frr); mode: "configured"
#   (bfdd.conf peers, both phases) or "bgp" (sessions from bgpd: iBGP across the
#   VRFs with "neighbor ... bfd", as the switch; vrf phase only); trace: bfdd
#   under LD_PRELOAD=sonic.routing.malloc-trace, its last dump printed.
{ pkgs, minutes ? 10, mode ? "configured", trace ? false
#   ppc64Pkgs (e.g. f.nixosConfigurations.as7760.pkgs): the daemons (and
#   malloc-trace) cross-built for the AS7760, run through qemu-user (binfmt)
#   inside the x86 VM; vtysh stays native.
, sonic ? import ../../../../packages/sonic { inherit pkgs; }, ppc64Pkgs ? null, heaptrack ? false
, target ? if ppc64Pkgs != null then import ../../../../packages/sonic { pkgs = ppc64Pkgs; } else sonic
, frr ? target.routing.frr }:
pkgs.testers.runNixOSTest {
  name = "sonic-bfdd-leak";
  nodes.dut = { lib, pkgs, ... }: {
    virtualisation.memorySize = 1536;
    virtualisation.cores = 2;
    boot.binfmt.emulatedSystems = lib.optional (ppc64Pkgs != null) "powerpc64-linux";
    virtualisation.vlans = [ 1 2 ];
    networking.firewall.enable = false;
    networking.interfaces = {
      eth1.ipv4.addresses = lib.mkForce [ { address = "10.1.0.1"; prefixLength = 24; } ];
      eth2.ipv4.addresses = lib.mkForce [ { address = "10.2.0.1"; prefixLength = 24; } ];
    };
    boot.kernelModules = [ "vrf" ];
    users.users.frr = { isSystemUser = true; group = "frr"; extraGroups = [ "frrvty" ]; };
    users.groups.frr = { };
    users.groups.frrvty = { };
    environment.systemPackages = [ sonic.routing.frr pkgs.procps ] ++ lib.optional heaptrack pkgs.heaptrack;
    environment.etc = {
      "frr/vtysh.conf".text = "no service integrated-vtysh-config\n";
      "frr/zebra.conf".text = "hostname dut\n";
      "frr/mgmtd.conf".text = "";
      "frr/bgpd.conf".text = ''
        router bgp 65100
         bgp router-id 10.255.0.1
         neighbor 10.0.0.1 remote-as 65100
         neighbor 10.0.0.1 bfd
        exit
        router bgp 65100 vrf Vrf-test
         bgp router-id 10.0.0.1
         neighbor 10.0.0.0 remote-as 65100
         neighbor 10.0.0.0 bfd
        exit
      '';
      "frr/bfdd.conf".text = if mode == "bgp" then "" else ''
        bfd
         peer 10.0.0.1 local-address 10.0.0.0 interface veth0
         !
         peer 10.0.0.0 local-address 10.0.0.1 interface veth1 vrf Vrf-test
         !
        !
      '';
    };
    systemd.tmpfiles.rules = [ "d /run/frr 0755 frr frr -" "d /var/lib/frr 0750 frr frr -" ];
    systemd.services = let
      d = name: args: {
        wantedBy = [ "multi-user.target" ];
        after = [ "loop-vrf.service" ] ++ lib.optional (name != "mgmtd") "frr-mgmtd.service"
          ++ lib.optional (name == "bfdd") "frr-zebra.service";
        requires = [ "loop-vrf.service" ];
        serviceConfig.ExecStart = "${frr}/libexec/frr/${name} ${args}";
        serviceConfig.Restart = "no";
      };
    in {
      loop-vrf = {
        wantedBy = [ "multi-user.target" ];
        path = [ pkgs.iproute2 ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
        script = ''
          ip link add Vrf-test type vrf table 1001; ip link set Vrf-test up
          ip link add veth0 type veth peer name veth1
          ip link set veth1 master Vrf-test
          ip addr add 10.0.0.0/31 dev veth0; ip addr add 10.0.0.1/31 dev veth1
          ip link set veth0 up; ip link set veth1 up
          # as SONiC's vrfmgrd: local table after the l3mdev rule (else a packet into
          # the VRF from the box's own default-VRF address is a martian)
          ip rule add pref 1001 table local; ip rule del pref 0
        '';
      };
      frr-mgmtd = d "mgmtd" "-P 0";
      frr-bgpd = lib.mkIf (mode == "bgp") ((d "bgpd" "-P 0") // { after = [ "frr-zebra.service" "frr-bfdd.service" ]; });
      frr-zebra = d "zebra" "-P 0";
      frr-bfdd = if heaptrack then (d "bfdd" "-P 0") // {
        serviceConfig.ExecStart = "${pkgs.heaptrack}/bin/heaptrack --record-only -o /var/lib/heaptrack/bfdd ${frr}/libexec/frr/bfdd -P 0";
        serviceConfig.StateDirectory = "heaptrack";
        serviceConfig.KillSignal = "SIGTERM";
      } else lib.recursiveUpdate (d "bfdd" "-P 0") (lib.optionalAttrs trace {
        environment = {
          LD_PRELOAD = "${target.routing.malloc-trace}/lib/libmalloc-trace.so";
          MALLOC_TRACE_OUT = "/var/lib/mt/bfdd.mt";
          MALLOC_TRACE_INTERVAL = "60";
        };
        serviceConfig.StateDirectory = "mt";
      });
    };
  };
  nodes.peer = { lib, ... }: {
    virtualisation.vlans = [ 1 2 ];
    networking.firewall.enable = false;
    networking.interfaces = {
      eth1.ipv4.addresses = lib.mkForce [ { address = "10.1.0.2"; prefixLength = 24; } ];
      eth2.ipv4.addresses = lib.mkForce [ { address = "10.2.0.2"; prefixLength = 24; } ];
    };
    services.frr = {
      bfdd.enable = true;
      config = ''
        bfd
         peer 10.1.0.1 local-address 10.1.0.2 interface eth1
         !
         peer 10.2.0.1 local-address 10.2.0.2 interface eth2
         !
        !
      '';
    };
  };
  testScript = ''
    MINUTES = ${toString minutes}
    HEAPTRACK = ${if heaptrack then "True" else "False"}
    MODE = "${mode}"
    TRACE = ${if trace then "True" else "False"}
  '' + builtins.readFile ./vm-bfdd-leak-test.py;
}
