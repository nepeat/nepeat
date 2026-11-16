# Our U-Boot in the DMC-12's boot SPI flash, installed from Linux: `dmc12-firmware` (see the script).
# The board DT exposes exactly three writable windows of CS0 for it (IPXE 0, IPXE 1, BOOT_MODE);
# everything else stays read-only. The payload is pinned: a rebuild that does not produce the
# rehearsed bytes fails the build instead of shipping an untested U-Boot (cross and native builds
# are not guaranteed identical). The read-only rest of CS0 is pinned too (factory layout, factory
# U-Boot intact; identical on u01 and p01): the tool refuses to write on anything else.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.dmc12.firmware;
  bp = pkgs.buildPackages;
  uboots = import ../../../../packages/uboot { pkgs = bp; };
  alpine = import ../../../../packages/uboot/alpine-v2 { pkgs = bp; inherit uboots; };
  alboot = ../../../../packages/alboot/alboot.py;

  # CS0 regions that stay read-only, as the factory left them (sha256 of each window). From the
  # verified SPI backups of u01 (factory) and p01 (2026-11-16), which agree on all three.
  pins = ''
    spi0-boot 38c62b92cc43868e8c4384af8e89cafc6eb679ffb03dbddc56a98492d9838236
    spi0-mid b88fa3d8184961430f0a4e35ac4c8569b9a9f928e71baf8a05b2ffab7b64975f
    spi0-high 251653f7be46fcc0e9398fb43ac7073c56d2b2cab5e8ac1aa3cd1d43fb5a53b9
  '';

  payloads = bp.runCommand "dmc12-firmware-payloads" { nativeBuildInputs = [ bp.python3 ]; } ''
    mkdir -p $out
    cp ${alpine.dmc12-stage3app}/u-boot-ipxe-08000000.fimg $out/ipxe.fimg
    got=$(sha256sum $out/ipxe.fimg | cut -d' ' -f1)
    if [ "$got" != "${cfg.ubootSha256}" ]; then
      echo "dmc12 firmware: the U-Boot payload is $got, not the pinned ${cfg.ubootSha256}." >&2
      echo "Rehearse the new build on a lab unit, then set hardware.dmc12.firmware.ubootSha256." >&2
      exit 1
    fi
    echo "$got" > $out/ipxe.sha256
    python3 -c "import re; print(re.search(rb'U-Boot (20[0-9.]+-[^ \0]+)', open('$out/ipxe.fimg','rb').read()).group(1).decode())" > $out/version
    # BOOT_MODE objects: data = (u32 0, u32 app), app = IPXE 0x110 | instance << 28 (as stage3 reads it)
    for n in 0 1; do
      python3 -c "import struct,sys; sys.stdout.buffer.write(struct.pack('<II', 0, ($n << 28) | 0x110).ljust(0x40, b'\0'))" > bm$n.data
      python3 ${alboot} wrap --id 0 --load 0 --version 2.60.4 bm$n.data $out/bm-ipxe$n.bin > /dev/null
      python3 -c "import sys; d=open('$out/bm-ipxe$n.bin','rb').read(); sys.stdout.buffer.write(d + b'\xff' * (4096 - len(d)))" | sha256sum | cut -d' ' -f1 > $out/bm-ipxe$n.sector.sha256
    done
    # the IPXE 1 object is byte-identical to what spi-upgrade.py installed on u01 (its journal)
    [ "$(sha256sum < $out/bm-ipxe1.bin | cut -d' ' -f1)" = 264864bfb772983c21ad8c7f7a84cd82b619ddb09b075d9aa072aaaf70489eb7 ]
    [ "$(sha256sum < $out/bm-ipxe0.bin | cut -d' ' -f1)" = db6049385481163d486c0276f4f55e82a39a0aa473a6f31ba4c8cb5196570c81 ]
    cp ${builtins.toFile "dmc12-firmware-pins" pins} $out/pins
  '';

  tool = pkgs.replaceVarsWith {
    src = ./dmc12-firmware.sh;
    dir = "bin";
    isExecutable = true;
    replacements = {
      bash = pkgs.bash;
      path = lib.makeBinPath [ pkgs.coreutils pkgs.diffutils pkgs.util-linux pkgs.mtdutils ];
      payloads = "${payloads}";
    };
  };
in
{
  options.hardware.dmc12.firmware = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "dmc12-firmware and the boot flash's writable IPXE/BOOT_MODE windows in the Linux DT.";
    };
    ubootSha256 = lib.mkOption {
      type = lib.types.str;
      # uboot-dmc12-stage3app at d55074f (control DT with al-fic + SPI TPM nodes; code as 929860b1)
      default = "94f29fd0b989a5bdfa0e8f0be6b9191c02f9f569c1b2c0ccc305bdeed6104c29";
      description = "sha256 of the rehearsed U-Boot IPXE object; the build fails on any other.";
    };
  };

  config = lib.mkIf cfg.enable {
    # After the env-window overlay (uboot-env), which already split CS0 into boot | env | tail:
    # boot (0..0xf90000) becomes spi0-boot | ipxe0 | boot-mode | spi0-mid | ipxe1 | spi0-high.
    hardware.deviceTree.overlays = lib.mkAfter [{
      name = "dmc12-firmware-windows";
      dtsText = ''
        /dts-v1/;
        /plugin/;
        / {
          compatible = "edgecore,dmc-12-48r-as0";
          fragment@0 {
            target-path = "/soc/io-bus@fc000000/spi@1882000/flash@0/partitions";
            __overlay__ {
              partition@0 { reg = <0x0 0x2c0000>; };
              partition@2c0000 { label = "ipxe0"; reg = <0x2c0000 0x80000>; };
              partition@340000 { label = "boot-mode"; reg = <0x340000 0x10000>; };
              partition@350000 { label = "spi0-mid"; reg = <0x350000 0x230000>; read-only; };
              partition@580000 { label = "ipxe1"; reg = <0x580000 0x80000>; };
              partition@600000 { label = "spi0-high"; reg = <0x600000 0x990000>; read-only; };
            };
          };
        };
      '';
    }];
    environment.systemPackages = [ tool ];
    system.build.dmc12FirmwarePayloads = payloads;
  };
}
