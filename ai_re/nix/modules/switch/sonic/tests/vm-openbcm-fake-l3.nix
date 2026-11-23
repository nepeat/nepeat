# L3 on our SAI's FAKE (virtual) backend under services.sonic (one VM, 3 GB):
# vrfmgrd + VRFOrch create a virtual router, a port bound to it gets its
# router interface, IP2ME/subnet and an APPL_DB route in that VRF; RouteOrch
# creates a next-hop group (ECMP) for a two-next-hop route and drops it again;
# FLEX_COUNTER_TABLE|PORT is seeded, syncd polls port stats, and the CLI's
# interface lists work in alias mode. Script: vm-l3-test.py.
#   nix build -L --impure --expr 'let f = builtins.getFlake "path:<ai_re>/nix";
#     pkgs = import f.inputs.nixpkgs { system = "x86_64-linux"; };
#     in import <ai_re>/nix/modules/switch/sonic/tests/vm-openbcm-fake-l3.nix { inherit pkgs;
#       openbcm.rustSai = <monorepo>/rust/sai/nix; }'
{ pkgs
, sonic ? import ../../../../packages/sonic { inherit pkgs; }
  # hardware.openbcm settings for the SAI under test (modules/switch/openbcm.nix):
  # { rustSai = <monorepo>/rust/sai/nix; } (required unless libsai / sairedis are given: the SAI
  # is the Rust one, outside this tree; the C++ sai-openbcm was deleted on 2026-11-23).
, openbcm ? { }
  # libsai / sairedis given explicitly win over that (rust/sai/nix's wrappers pass them).
, libsai ? null
, sairedis ? null }:
let sai = import ./openbcm-sai.nix { inherit sonic openbcm libsai sairedis; }; in
pkgs.testers.runNixOSTest {
  name = "sonic-l3-openbcm-fake";
  nodes.switch = { lib, pkgs, options, ... }: {
    imports = [ ./common.nix sai.module ];
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    networking.firewall.enable = false;
    environment.systemPackages = [ sonic.swss ];   # swssconfig: APPL_DB routes without FRR
    services.sonic = {
      packages = sonic;
      breakout = lib.mkForce { };
      sai = {
        kind = "vendor";
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
      settings.PORT = lib.genAttrs [ "Ethernet1/1" "Ethernet3/1" "Ethernet4/1" "Ethernet32/1" ]
        (_: { admin_status = "up"; });
    } // lib.optionalAttrs (options.services.sonic ? frr) {
      frr.enable = false;   # routes come from APPL_DB here (swssconfig), not FRR
    };
  };
  testScript = builtins.readFile ./vm-l3-test.py;
}
