# Packages that only need the build host's pkgs (bootloaders, host tools).
# Packages tied to a target system's kernel/config are built in ../configs.
{ pkgs }:
let
  uboots = import ./uboot { inherit pkgs; };
  ubootsAlpine = import ./uboot/alpine-v2 { inherit pkgs uboots; };
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

  # Upstream U-Boot for Alpine V2 (arm64), same series, alpine-v2 + dmc12
  # scopes; see ./uboot/alpine-v2/default.nix. RAM only, started by the
  # vendor U-Boot: u-boot.uimage for "bootm", u-boot.bin for "go"/booti.
  uboot-dmc12-chainload = ubootsAlpine.dmc12-chainload;
  uboot-alpine-v2-qemu = ubootsAlpine.qemu-virt;     # test fixture build
  uboot-alpine-v2-tests = import ./uboot/alpine-v2/tests.nix { inherit pkgs; alpine = ubootsAlpine; };

  # Offline tests (native): monitoring exporter and the board metrics producers.
  node-exporter-lite-test = pkgs.callPackage ./node-exporter-lite/test.nix { };
  as7760-metrics-test = pkgs.callPackage ./as7760-metrics/test.nix { };
  dmc12-metrics-test = pkgs.callPackage ./dmc12-metrics/test.nix { };
}
