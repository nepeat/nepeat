# Packages that only need the build host's pkgs (bootloaders, host tools).
# Packages tied to a target system's kernel/config are built in ../configs.
{ pkgs }:
let
  uboots = import ./uboot { inherit pkgs; };
  sonic = import ./sonic { inherit pkgs; };
in {
  # SONiC switch-state stack as native packages; see ./sonic/default.nix.
  sonic-swss-common = sonic.swss-common;

  # Upstream U-Boot for T208x boards; see ./uboot/default.nix for the series.
  uboot-t2080rdb = uboots.t2080rdb-nor;            # stock reference build, never flash
  uboot-as7760-nor = uboots.as7760-nor;            # boot NOR image (mtd7)
  uboot-as7760-chainload = uboots.as7760-chainload; # RAM build: "go 0x00200000" test stage
  uboot-qemu-ppce500 = uboots.qemu-ppce500;
  uboot-tests = import ./uboot/tests.nix { inherit pkgs uboots; };
}
