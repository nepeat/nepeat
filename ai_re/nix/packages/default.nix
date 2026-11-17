# Packages that only need the build host's pkgs (bootloaders, host tools).
# Packages tied to a target system's kernel/config are built in ../configs.
{ pkgs }:
let
  uboots = import ./uboot { inherit pkgs; };
  ubootsAlpine = import ./uboot/alpine-v2 { inherit pkgs uboots; };
  ubootsAlpineV3 = import ./uboot/alpine-v3 { inherit pkgs uboots; };
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
  # + external PCIe init, MUIO/CCU, read-only SPI NOR (RAM test build), and
  # the same u-boot.bin wrapped as an al-boot stage3 application (not flashed).
  uboot-dmc12-selfinit = ubootsAlpine.dmc12-selfinit;
  uboot-dmc12-stage3app = ubootsAlpine.dmc12-stage3app;
  uboot-alpine-v2-qemu = ubootsAlpine.qemu-virt;     # test fixture build
  uboot-alpine-v2-tests = import ./uboot/alpine-v2/tests.nix { inherit pkgs; alpine = ubootsAlpine; };

  # Upstream U-Boot for Alpine V3 (arm64), same series, alpine + alpine-v3 +
  # cs8210 scopes; see ./uboot/alpine-v3/default.nix. RAM only, started by
  # the vendor U-Boot (loadx, then booti/bootm/go).
  uboot-cs8210 = ubootsAlpineV3.cs8210;
  # Installer: same, boot SPI writable only in the scheme A' windows (0027/0028),
  # for devices/cs8210-32x/scripts/spi_upgrade.py; its sandbox twin for rehearsals.
  uboot-cs8210-installer = ubootsAlpineV3.cs8210-installer;
  uboot-sandbox-cs8210-installer = ubootsAlpineV3.sandbox-installer;
  # Standalone: + the vendor board init (PCIe RC, MUIO, GPIO, PLL, MAC), SPI env;
  # the stage3 app object of it for scheme A' (devices/cs8210-32x/u-boot-port.md).
  uboot-cs8210-selfinit = ubootsAlpineV3.cs8210-selfinit;
  uboot-cs8210-stage3app = ubootsAlpineV3.cs8210-stage3app;
  uboot-alpine-v3-qemu = ubootsAlpineV3.qemu-virt;  # test fixture build
  uboot-alpine-v3-tests = import ./uboot/alpine-v3/tests.nix { inherit pkgs; alpine = ubootsAlpineV3; };

  # Offline tests (native): monitoring exporter and the board metrics producers.
  node-exporter-lite-test = pkgs.callPackage ./node-exporter-lite/test.nix { };
  as7760-metrics-test = pkgs.callPackage ./as7760-metrics/test.nix { };
  dmc12-metrics-test = pkgs.callPackage ./dmc12-metrics/test.nix { };
}
