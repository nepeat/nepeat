# Experiment: nepeat/nix's hardware.cs8210.sonic (ai_re services.sonic) with
# syncd = the pinned upstream sairedis (amd64) on Marvell's isai 6.2.0 under
# qemu-ipd, instead of our sai-teralynx. Imported by no system config; used by
# extendModules in throwaway expressions to build the units' store paths, which
# then run by hand (devices/cs8210-32x/sai-qemu.md, "syncd").
#   (nixosConfigurations.cs8210.extendModules { modules = [ (import ./sonic-isai.nix { qemuIpd = ...; }) ]; })
# Needs ipd with legacy_abi=0 and the IFCS yaml + inno.config.yaml at runtimeDir.
{ qemuIpd, runtimeDir ? "/run/isai-sonic", cages ? [ 1 16 ] }:
{ lib, ... }:
{
  hardware.cs8210.sonic = {
    enable = lib.mkForce true;
    lineMedia = { "1" = "copper"; "16" = "copper"; };
    # the derived spec needs a port in every cage 1-16; the loop's spec as credo-up
    credoPorts = "1:copper_1x400G:fec-def;2-15:optics_1x400G:fec-def;16:copper_1x400G:fec-def";
  };
  # only these cages get a port (CONFIG_DB and the generated IFCS yaml)
  services.sonic.breakout = lib.genAttrs
    (map toString (lib.filter (c: !(lib.elem c cages)) (lib.range 1 32))) (_: [ ]);
  services.sonic.sai = {
    package = lib.mkForce qemuIpd.syncd-runner;
    # the guest's libraries come from its RUNPATHs; an arm64 LD_LIBRARY_PATH would reach the guest
    libraryPath = lib.mkForce [ ];
    profile = lib.mkForce { SAI_INIT_CONFIG_FILE = "${runtimeDir}/inno.config.yaml"; };
  };
}
