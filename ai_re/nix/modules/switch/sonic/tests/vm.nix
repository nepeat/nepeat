# NixOS VM test of services.sonic with the virtual SAI: services up, ports
# (incl. an explicit 4x25G split) in CONFIG/APPL/ASIC_DB and the kernel, the
# CLI in alias mode, VLAN, a PortChannel with LACP to a peer VM, the config
# ownership rules (enforce conflict, seed sticks, host tables, rebuild apply).
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm.nix { inherit pkgs; }'
{ pkgs }:
pkgs.testers.runNixOSTest {
  name = "sonic-control-plane";
  nodes.switch = { lib, ... }: {
    imports = [ ./common.nix ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    virtualisation.vlans = [ 1 2 ];
    # eth1/eth2 carry Ethernet5/1 and Ethernet6/1 for the virtual SAI: no host IP.
    networking.interfaces = lib.genAttrs [ "eth1" "eth2" ] (_: {
      ipv4.addresses = lib.mkForce [ ];
      ipv6.addresses = lib.mkForce [ ];
    });
    networking.firewall.enable = false;
    services.openssh.enable = true;
    # Declared in Nix (enforced by defaultMode): must survive everything.
    services.sonic.settings.VLAN.Vlan200.vlanid = "200";
    # nixos-rebuild switch to this: seed change (applied), seed change over a
    # user value (not applied), new enforced key (applied).
    specialisation.changed.configuration = {
      services.sonic.settings = {
        PORT."Ethernet7/1".mtu = "1500";
        PORT."Ethernet4/1".mtu = "4000";
        VLAN.Vlan300.vlanid = "300";
      };
    };
  };
  nodes.peer = { lib, pkgs, ... }: {
    virtualisation.vlans = [ 1 2 ];
    networking.interfaces = lib.genAttrs [ "eth1" "eth2" ] (_: {
      ipv4.addresses = lib.mkForce [ ];
      ipv6.addresses = lib.mkForce [ ];
    }) // {
      bond0.ipv4.addresses = [ { address = "10.0.0.2"; prefixLength = 24; } ];
    };
    networking.bonds.bond0 = {
      interfaces = [ "eth1" "eth2" ];
      driverOptions = { mode = "802.3ad"; miimon = "100"; lacp_rate = "fast"; };
    };
    networking.firewall.enable = false;
    # Linux 802.3ad needs a slave speed/duplex; virtio-net reports none.
    systemd.services.virtio-link-speed = {
      wantedBy = [ "bond0-netdev.service" ];
      before = [ "bond0-netdev.service" ];
      after = [ "sys-subsystem-net-devices-eth1.device" "sys-subsystem-net-devices-eth2.device" ];
      wants = [ "sys-subsystem-net-devices-eth1.device" "sys-subsystem-net-devices-eth2.device" ];
      path = [ pkgs.ethtool ];
      serviceConfig.Type = "oneshot";
      script = "for i in eth1 eth2; do ethtool -s $i speed 10000 duplex full; done";
    };
  };

  testScript = builtins.readFile ./vm-test.py;
}
