# "sonic-nix reload" (config reload) restarts the whole stack around the CONFIG_DB flush (one VM, 3 GB,
# virtual SAI, ~6 min). Every unit PartOf sonic.target must be down while the loader writes CONFIG_DB and come
# back after it; a saved VRF binding must be back after the reload. Negative control: the pre-fix sequence
# ("systemctl stop sonic.target", load, start: stop returns once the target's own job is done, the start
# replaces the PartOf units' still-queued stop jobs) shows units that never went down. reload-probe is a unit
# that stops slowly on demand (/run/reload-probe-slow), ordered after orchagent like the managers, so syncd
# and orchagent wait for it on a stop: what the CS8210's slow stops (lldpd, xcvrd, Credo) did there
# (devices/cs8210-32x/evidence/rust-h4/12-target-stopstart-cause.txt). Script: vm-reload-test.py.
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-reload.nix { inherit pkgs; }'
{ pkgs }:
pkgs.testers.runNixOSTest {
  name = "sonic-reload";
  nodes.switch = { lib, pkgs, ... }: {
    imports = [ ./common.nix ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    virtualisation.vlans = [ 1 2 ];
    # eth1/eth2 carry Ethernet5/1 and Ethernet6/1 for the virtual SAI (common.nix): no host IP.
    networking.interfaces = lib.genAttrs [ "eth1" "eth2" ] (_: {
      ipv4.addresses = lib.mkForce [ ];
      ipv6.addresses = lib.mkForce [ ];
    });
    networking.firewall.enable = false;
    systemd.services.reload-probe = {
      description = "test: a sonic.target unit that stops slowly while /run/reload-probe-slow exists";
      wantedBy = [ "sonic.target" ];
      partOf = [ "sonic.target" ];
      after = [ "sonic-orchagent.service" ];
      serviceConfig = {
        ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
        ExecStop = "${pkgs.bash}/bin/sh -c 'if [ -e /run/reload-probe-slow ]; then ${pkgs.coreutils}/bin/sleep 8; fi'";
      };
    };
  };
  testScript = builtins.readFile ./vm-reload-test.py;
}
