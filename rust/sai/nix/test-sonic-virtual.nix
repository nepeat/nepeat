# The full SONiC control plane (syncd, orchagent, the managers, teamd's LACP to a peer VM) on
# the Rust libsai's virtual backend: ai_re's vm-openbcm-fake.nix (passed in as `fakeTest`) with
# libsai replaced through its own parameters, and nothing else: its SAI profile (the C++ keys,
# SAI_OPENBCM_BACKEND=fake, _FAKE_PORTS / _LANES / _NETDEV=tap / _WIRE, which the Rust library
# reads) and its script vm-test.py (the syncd log line "switch created: 32 front-panel ports,
# backend fake" included) are the original's, check for check.
#   import ./test-sonic-virtual.nix { pkgs = p; sonic = <ai_re sonic set>; libsai = <./package.nix>;
#     fakeTest = <ai_re>/nix/modules/switch/sonic/tests/vm-openbcm-fake.nix; }
{ pkgs
, sonic
, libsai
, fakeTest
, sairedis ? sonic.sairedis-openbcm libsai
}:
let
  inherit (pkgs) lib;
in
(import fakeTest { inherit pkgs sonic libsai sairedis; }).extend {
  modules = [
    { name = lib.mkForce "sai-rs-sonic-control-plane"; }
  ];
}
