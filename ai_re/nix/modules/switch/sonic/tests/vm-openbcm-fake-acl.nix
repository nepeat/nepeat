# ACL and CoPP policing on sai-openbcm's FAKE backend under services.sonic
# (one VM, 3 GB): CoppOrch's policers reach the SAI (no NOT_IMPLEMENTED) and
# become CoPP field entries; an L3 ACL table + DROP rule written to CONFIG_DB
# (and one via `config acl add table`) become an ACL table, a table group
# bound to the port, an entry with its counter (polled into COUNTERS_DB), and
# go away again. Script: vm-acl-test.py.
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-openbcm-fake-acl.nix { inherit pkgs; }'
{ pkgs
, sonic ? import ../../../../packages/sonic { inherit pkgs; }
, libsai ? pkgs.callPackage ../../../../packages/sai-openbcm { inherit sonic; }
, sairedis ? sonic.sairedis-openbcm libsai }:
pkgs.testers.runNixOSTest {
  name = "sonic-acl-openbcm-fake";
  nodes.switch = { lib, options, ... }: {
    imports = [ ./common.nix ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    networking.firewall.enable = false;
    services.sonic = {
      packages = sonic;
      breakout = lib.mkForce { };
      sai = {
        kind = "vendor";
        package = sairedis;
        libraryPath = [ "${libsai}/lib" ];
        vs.interfaces = lib.mkForce { };
        profile = {
          SAI_OPENBCM_BACKEND = "fake";
          SAI_OPENBCM_FAKE_PORTS = "32";
          SAI_OPENBCM_FAKE_LANES = "4";
          SAI_OPENBCM_FAKE_NETDEV = "tap";
          SAI_OPENBCM_LOG_STDERR = "1";
        };
      };
      kernelModules = [ "team" "team_mode_loadbalance" "bridge" "8021q" "dummy" "tun" ];
      settings.PORT = lib.genAttrs [ "Ethernet3/1" "Ethernet4/1" ] (_: { admin_status = "up"; });
    } // lib.optionalAttrs (options.services.sonic ? frr) {
      frr.enable = false;
    };
  };
  testScript = builtins.readFile ./vm-acl-test.py;
}
