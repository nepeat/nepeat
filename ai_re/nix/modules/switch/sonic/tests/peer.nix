# LACP peer of the VM tests: eth1 + eth2 bonded (802.3ad, fast), 10.0.0.2/24.
{ lib, pkgs, ... }: {
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
}
