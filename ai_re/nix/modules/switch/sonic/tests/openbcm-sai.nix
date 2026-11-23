# The SAI of the fake-backend VM tests (vm-openbcm-fake*.nix): libsai and the syncd built
# against it come from hardware.openbcm (../../openbcm.nix, the option the boards use; not
# enabled: a VM has no SDK modules), with the settings the test was given and without the SDK
# (fake / virtual backend). libsai / sairedis, when given, are used as they are.
{ sonic, openbcm, libsai, sairedis }:
{
  nameSuffix = if (openbcm.implementation or "cpp") == "cpp" then "" else "-${openbcm.implementation}";
  module = { config, ... }:
    let so = if libsai != null then libsai else config.hardware.openbcm.package; in {
      imports = [ ../../openbcm.nix ];
      hardware.openbcm = openbcm // { sdk = false; };
      services.sonic.sai = {
        package = if sairedis != null then sairedis else sonic.sairedis-openbcm so;
        libraryPath = [ "${so}/lib" ];
      };
    };
}
