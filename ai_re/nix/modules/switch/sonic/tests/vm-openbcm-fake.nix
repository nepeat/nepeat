# The same VM test as vm.nix (vm-test.py, SAI = "openbcm-fake"), with syncd on
# sai-openbcm's FAKE backend instead of the virtual SAI: what orchagent and
# the managers do against the thin SAI the AS7760 runs, without the ASIC.
# The fake has the AS7760's 32 cages x 4 lanes (lanes 1..128, the same sets
# as port-maps/accton-as7712-32x.nix), no breakout (the SDK takes that from
# config.bcm), and real Linux netdevs per hostif (TAP, SAI_OPENBCM_FAKE_NETDEV).
# Its "wire" (SAI_OPENBCM_FAKE_WIRE) carries the CPU path of two ports to the
# peer VM's NICs, so teamd's LACP and a ping reach the peer.
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-openbcm-fake.nix { inherit pkgs; }'
{ pkgs
, sonic ? import ../../../../packages/sonic { inherit pkgs; }
  # hardware.openbcm settings choosing the SAI under test (modules/switch/openbcm.nix), e.g.
  # { implementation = "rust"; rustSai = <monorepo>/rust/sai/nix; }; default: the C++ sai-openbcm.
, openbcm ? { }
  # libsai / sairedis given explicitly win over that (rust/sai/nix's wrappers pass them).
, libsai ? null
  # syncd built against libsai (default); an older build also works while
  # iterating on libsai alone (libraryPath puts this libsai first).
, sairedis ? null }:
let sai = import ./openbcm-sai.nix { inherit sonic openbcm libsai sairedis; }; in
pkgs.testers.runNixOSTest {
  name = "sonic-control-plane-openbcm-fake" + sai.nameSuffix;
  nodes.switch = { lib, pkgs, ... }: {
    imports = [ ./common.nix sai.module ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    virtualisation.vlans = [ 1 2 ];
    networking.interfaces = lib.genAttrs [ "eth1" "eth2" ] (_: {
      ipv4.addresses = lib.mkForce [ ];
      ipv6.addresses = lib.mkForce [ ];
    });
    networking.firewall.enable = false;
    services.openssh.enable = true;
    services.sonic = {
      packages = sonic;
      breakout = lib.mkForce { };          # the fake (like the SDK) has no SAI breakout
      sai = {
        kind = "vendor";
        vs.interfaces = lib.mkForce { };
        profile = {
          SAI_OPENBCM_BACKEND = "fake";
          SAI_OPENBCM_FAKE_PORTS = "32";
          SAI_OPENBCM_FAKE_LANES = "4";
          SAI_OPENBCM_FAKE_NETDEV = "tap";
          SAI_OPENBCM_FAKE_WIRE = "Ethernet16:eth1,Ethernet20:eth2";
          SAI_OPENBCM_LOG_STDERR = "1";
        };
        conflicts = [ "fake-asic-temp.service" ];
        restartOnStop = [ "fake-asic-temp.service" ];
      };
      kernelModules = [ "team" "team_mode_loadbalance" "bridge" "8021q" "dummy" "tun" ];
      settings.VLAN.Vlan200.vlanid = "200";
      # vm-test.py's enforce-conflict flow changes Ethernet1/1's speed; the vs
      # test has it enforced by the breakout, here by an explicit mode.
      fieldModes.PORT."Ethernet1/1".speed = "enforce";
    };
    systemd.services.fake-asic-temp = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
    };
    specialisation.changed.configuration = {
      services.sonic.settings = {
        PORT."Ethernet7/1".mtu = "1500";
        PORT."Ethernet4/1".mtu = "4000";
        VLAN.Vlan300.vlanid = "300";
      };
    };
  };
  nodes.peer = ./peer.nix;
  testScript = ''
    SAI = "openbcm-fake"
  '' + builtins.readFile ./vm-test.py;
}
