# services.sonic.macsec start-up on the virtual SAI (one VM, 3 GB): MKA must come up by itself between two ports of
# one switch, and an MKA session (wpa_supplicant) must start with the address its port keeps and start again when
# the address changes. Ethernet5/1 and Ethernet6/1 are cabled to each other (eth1 and eth2 on one segment), both
# with a MACsec profile; Ethernet6/1 has a CONFIG_DB INTERFACE mac_addr, which intfmgrd puts on the netdev some
# time after the port is ready. A session started before that runs with the switch address at both ends ("duplicate
# MAC detected", no SAs): what left MKA down on the AS7772 after stack restarts
# (devices/edge-core-as7772-32x/macsec.md "Under SONiC"; packages/sonic/swss/patches/0003).
# Checked at boot, after a syncd restart (new netdevs), after "ip link set address" and after a CONFIG_DB mac_addr
# change: the address each session was started with (macsecmgrd's log and wpa_supplicant's own answer), SAs in
# STATE_DB with the SCIs of the addresses the ports have, no "duplicate MAC" from the KaYs, and a port whose
# address did not change keeps its wpa_supplicant. Not covered: frames through the MACsec devices (the virtual
# SAI's kernel macsec data path is not exercised) and netdevs that outlive syncd (the virtual SAI's TAP devices go
# with the process; on the switch the old ones stay until the new syncd replaces them). Script: vm-macsec-test.py.
# The CAK is a test value written by a unit in the VM (cakFile is always outside the store).
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-macsec.nix { inherit pkgs; }'
{ pkgs }:
pkgs.testers.runNixOSTest {
  name = "sonic-macsec-start-order";
  nodes.switch = { lib, options, config, ... }: {
    imports = [ ./common.nix ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    # eth1 and eth2 carry Ethernet5/1 and Ethernet6/1 (common.nix) and sit on ONE segment: the two ports are
    # cabled to each other, both ends of the link on this switch, as the AS7772's DAC loops.
    virtualisation.vlans = [ 1 1 ];
    networking.interfaces = lib.genAttrs [ "eth1" "eth2" ] (_: {
      ipv4.addresses = lib.mkForce [ ];
      ipv6.addresses = lib.mkForce [ ];
    });
    networking.firewall.enable = false;
    services.sonic = {
      kernelModules = [ "macsec" ];
      settings.PORT = lib.genAttrs [ "Ethernet5/1" "Ethernet6/1" ] (_: { admin_status = "up"; });
      # Ethernet6/1 (Ethernet20) gets its own address; Ethernet5/1 (Ethernet16) keeps the switch's.
      settings.INTERFACE."Ethernet6/1".mac_addr = "52:54:00:aa:bb:02";
      macsec = {
        enable = true;
        profiles.test = {
          cakFile = "/var/lib/sonic-macsec/test.cak";
          ckn = "766d2d6d61637365632d746573740000000000000000000000000000000000ff";
        };
        ports = { "Ethernet5/1" = "test"; "Ethernet6/1" = "test"; };
      };
    } // lib.optionalAttrs (options.services.sonic ? frr) {
      frr.enable = false;
    };
    systemd.services.test-macsec-cak = {
      wantedBy = [ "sonic-macsec-config.service" ];
      before = [ "sonic-macsec-config.service" ];
      serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
      script = ''
        install -d -m 700 /var/lib/sonic-macsec
        (umask 077; echo 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f > /var/lib/sonic-macsec/test.cak)
      '';
    };
    environment.systemPackages = [ config.services.sonic.packages.wpa-supplicant ];
  };
  testScript = builtins.readFile ./vm-macsec-test.py;
}
