# Shared switch config of the VM tests: AS7760/AS7712 port map, cage 1 split
# 4x25G (explicit), two ports wired to the peer VM for the PortChannel.
{ lib, ... }: {
  imports = [ ../default.nix ];
  services.sonic = {
    enable = true;
    platform = "x86_64-accton_as7760_32x-r0";
    hwsku = "Accton-AS7760-32X";
    macAddress = "52:54:00:aa:bb:01";
    cages = import ../port-maps/accton-as7712-32x.nix;
    breakout."1" = [
      { alias = "Ethernet1/1"; lanes = [ 49 ]; speed = 25000; }
      { alias = "Ethernet1/2"; lanes = [ 50 ]; speed = 25000; }
      { alias = "Ethernet1/3"; lanes = [ 51 ]; speed = 25000; }
      { alias = "Ethernet1/4"; lanes = [ 52 ]; speed = 25000; }
    ];
    sai.vs.interfaces = { "Ethernet5/1" = "eth1"; "Ethernet6/1" = "eth2"; };
  };
}
