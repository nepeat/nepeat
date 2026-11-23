# The full SONiC control plane (syncd, orchagent, the managers, teamd's LACP to a peer VM) on
# the Rust libsai's virtual backend: ai_re's vm-openbcm-fake.nix (passed in as `fakeTest`) with
# libsai replaced through its own parameters, and through the test's `extend`:
#   - the SAI profile: the C++ keys (SAI_OPENBCM_BACKEND=fake, _FAKE_NETDEV=tap, _FAKE_WIRE)
#     become the Rust library's (SAI_RS_BACKEND=virtual, SAI_RS_VIRTUAL_NETDEV=tap,
#     SAI_RS_VIRTUAL_WIRE), same values;
#   - the script: vm-test.py next to fakeTest, read from there, with exactly two strings
#     replaced (each must occur, else evaluation fails): the profile line it looks for, and the
#     syncd log line that names the backend. Everything else, check for check, is the original.
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
  swap = text: { from, to }:
    assert lib.assertMsg (lib.hasInfix from text) "test-sonic-virtual.nix: vm-test.py no longer has: ${from}";
    builtins.replaceStrings [ from ] [ to ] text;
  script = lib.foldl' swap (builtins.readFile (dirOf fakeTest + "/vm-test.py")) [
    { from = ''"SAI_OPENBCM_BACKEND=fake" in sp''; to = ''"SAI_RS_BACKEND=virtual" in sp''; }
    { from = "switch created: 32 front-panel ports, backend fake";
      to = "VIRTUAL backend selected: no hardware is programmed; switch created: 32 front-panel ports"; }
  ];
in
(import fakeTest { inherit pkgs sonic libsai sairedis; }).extend {
  modules = [
    {
      name = lib.mkForce "sai-rs-sonic-control-plane";
      nodes.switch = {
        services.sonic.sai.profile = lib.mkForce {
          SAI_RS_BACKEND = "virtual";
          SAI_RS_VIRTUAL_PORTS = "32";
          SAI_RS_VIRTUAL_LANES = "4";
          SAI_RS_VIRTUAL_NETDEV = "tap";
          SAI_RS_VIRTUAL_WIRE = "Ethernet16:eth1,Ethernet20:eth2";
        };
      };
      testScript = lib.mkForce (''
        SAI = "openbcm-fake"
      '' + script);
    }
  ];
}
