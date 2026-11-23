# ai_re's vm-openbcm-fake-acl.nix (passed in as `fakeTest`) on the Rust libsai's virtual backend, unchanged
# but for libsai (its own parameters): the SAI profile is the original's (the C++ keys,
# SAI_OPENBCM_BACKEND=fake / _FAKE_PORTS / _FAKE_LANES / _FAKE_NETDEV=tap, which the Rust library
# reads), and so is the script (vm-acl-test.py).
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
    { name = lib.mkForce "sai-rs-sonic-acl"; }
  ];
}
