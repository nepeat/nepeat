# ai_re's vm-openbcm-fake-acl.nix (passed in as `fakeTest`) on the Rust libsai's virtual backend, unchanged
# but for libsai (its own parameters) and the SAI profile (the test's `extend`): the C++ keys
# SAI_OPENBCM_BACKEND=fake / _FAKE_PORTS / _FAKE_LANES / _FAKE_NETDEV=tap become
# SAI_RS_BACKEND=virtual / SAI_RS_VIRTUAL_PORTS / _LANES / _NETDEV=tap. The script (vm-acl-test.py) is the
# original's.
# ACL is not in the Rust slice: this target shows how far orchagent gets (README, "Parity").
#   import ./test-sonic-virtual-acl.nix { pkgs = p; sonic = <ai_re sonic set>; libsai = <./package.nix>;
#     fakeTest = <ai_re>/nix/modules/switch/sonic/tests/vm-openbcm-fake-acl.nix; }
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
    {
      name = lib.mkForce "sai-rs-sonic-acl";
      nodes.switch = {
        services.sonic.sai.profile = lib.mkForce {
          SAI_RS_BACKEND = "virtual";
          SAI_RS_VIRTUAL_PORTS = "32";
          SAI_RS_VIRTUAL_LANES = "4";
          SAI_RS_VIRTUAL_NETDEV = "tap";
        };
      };
    }
  ];
}
