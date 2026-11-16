# dmc12-firmware (our U-Boot into a DMC-12's IPXE slots + BOOT_MODE, from Linux) and its pinned
# payloads. Used by modules/boards/dmc12/firmware (bootLoader from the host config) and exported by
# the root flake as packages.aarch64-linux.dmc12-firmware (bootLoader = "auto": read from /boot), so
#   nix run --refresh 'github:nepeat/nepeat?dir=ai_re#dmc12-firmware' -- status
# works on any DMC-12. The tool still needs the firmware windows in the running DT (the board module).
{ pkgs, lib ? pkgs.lib, bootLoader ? "auto",
  # uboot-dmc12-stage3app with the reproducible CC_VERSION_STRING: identical from x86 cross and
  # native aarch64 builds (proven 2026-11-16, booted on u01). Earlier: 94f29fd0 (cross only).
  ubootSha256 ? "d3df4b8088c116d0530dccfe4f3d7926535953d0cafae326030d6fa64cfb5c75" }:
let
  bp = pkgs.buildPackages;
  uboots = import ../uboot { pkgs = bp; };
  alpine = import ../uboot/alpine-v2 { pkgs = bp; inherit uboots; };
  alboot = ../alboot/alboot.py;

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
    if [ "$got" != "${ubootSha256}" ]; then
      echo "dmc12 firmware: the U-Boot payload is $got, not the pinned ${ubootSha256}." >&2
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
    src = ./dmc12-firmware;
    dir = "bin";
    isExecutable = true;
    meta.mainProgram = "dmc12-firmware";
    replacements = {
      bash = pkgs.bash;
      path = lib.makeBinPath [ pkgs.coreutils pkgs.diffutils pkgs.gnugrep pkgs.util-linux pkgs.mtdutils ];
      payloads = "${payloads}";
      inherit bootLoader;
    };
  };
in
{ inherit tool payloads ubootSha256; }
