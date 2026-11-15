{ pkgs, alpine }:
# Checks for the Alpine V2 chainload U-Boot that need no board:
#  static: AArch64 ELF linked at the load address, Linux Image header,
#          uImage header (kernel type, load = entry = loadAddr), footprint
#          clear of the vendor initrd staging, no SMC/HVC anywhere (the
#          firmware's PSCI v0.1 has no SYSTEM_RESET), RAM-only/safety config,
#          DMC-12 control DT contents (memory, firmware ranges, resets).
#  qemu:   ./qemu-test.py on QEMU virt with the same board code.
let
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  binutils = cross.buildPackages.binutils-unwrapped;
  d = alpine.dmc12-chainload;
  static = pkgs.runCommand "alpine-v2-uboot-static-checks" {
    nativeBuildInputs = [ binutils pkgs.python3 pkgs.gnugrep pkgs.dtc ];
  } ''
    set -euo pipefail
    h=$(aarch64-unknown-linux-gnu-readelf -h ${d}/u-boot)
    grep -q 'Class:.*ELF64' <<<"$h"; grep -q 'Machine:.*AArch64' <<<"$h"
    grep -qi 'Entry point address:.*0x8000000$' <<<"$h" || { grep Entry <<<"$h"; exit 1; }
    aarch64-unknown-linux-gnu-objdump -d ${d}/u-boot > dis.txt
    n=$(grep -cE '\s(smc|hvc)\s' dis.txt || true)
    [ "$n" = 0 ] || { echo "$n smc/hvc instructions"; grep -E '\s(smc|hvc)\s' dis.txt | head; exit 1; }
    echo "elf: ok (AArch64, entry 0x08000000, no smc/hvc)"

    python3 - ${d}/u-boot.bin ${alpine.loadAddr} <<'PY'
    import struct, sys
    b = open(sys.argv[1], "rb").read(); load = int(sys.argv[2], 16)
    assert b[0x38:0x3c] == b"ARM\x64", "no Linux Image magic"
    text_off, image_size, flags = struct.unpack_from("<QQQ", b, 8)
    assert text_off == load, hex(text_off)
    assert flags & 0b1010 == 0b1010, hex(flags)   # 4K pages, place anywhere
    end = load + image_size
    # vendor initramfs staging (loadaddr_initramfs) starts at 0x0fd00000
    assert end < 0x0fd00000 and image_size < 0x200000, hex(end)
    print(f"image: ok ({len(b)} B file, {image_size:#x} B in RAM, {load:#x}-{end:#x})")
    PY

    u=${d}/u-boot.uimage.txt; cat $u
    grep -q 'Image Type:.*AArch64 Linux Kernel Image (uncompressed)' $u
    grep -q 'Load Address: 08000000' $u; grep -q 'Entry Point:  08000000' $u
    echo "uimage: ok"

    c=${d}/.config
    for k in NET SPI_FLASH CMD_SF DM_SPI CMD_SAVEENV EFI_LOADER PSCI_RESET SYSRESET_PSCI \
             WATCHDOG_AUTOSTART MMC CMD_EXT4_WRITE FAT_WRITE CMD_MTD ENV_IS_IN_SPI_FLASH \
             ARMV8_MULTIENTRY ARMV8_PSCI GICV3 ARM_ERRATA_828024 ARMV8_SET_SMPEN; do
      ! grep -q "^CONFIG_$k=y" $c || { echo "config: CONFIG_$k is on"; exit 1; }
    done
    for kv in TEXT_BASE=0x08000000 POSITION_INDEPENDENT=y ENV_IS_NOWHERE=y SYSRESET_DMC12_CPLD=y \
              SYSRESET_WATCHDOG=y WDT_SP805=y CMD_BOOTI=y CMD_SYSBOOT=y BOOTMETH_EXTLINUX=y \
              PCIE_ECAM_GENERIC=y AHCI_PCI=y SCSI=y CMD_EXT4=y CMD_FAT=y OF_BOARD_SETUP=y \
              NR_DRAM_BANKS=2 'DEFAULT_FDT_FILE="amazon/alpine-v2-dmc12.dtb"' \
              'BOOTCOMMAND="bootflow scan -lb"'; do
      grep -qx "CONFIG_$kv" $c || { echo "config: CONFIG_$kv missing"; exit 1; }
    done
    echo "config: ok (RAM only: no SPI flash, no env storage, no network, no PSCI reset)"

    t=${d}/u-boot.dtb
    [ "$(fdtget -t x $t /memory@0 reg)" = "0 0 0 c0000000 2 0 0 40000000" ]
    for r in "al-boot@0:0 0 0 100000" "al-boot@2000000:0 2000000 0 100000" \
             "al-boot@b0000000:0 b0000000 0 8000000" "al-boot@b8000000:0 b8000000 0 8000000"; do
      [ "$(fdtget -t x $t /reserved-memory/''${r%%:*} reg)" = "''${r#*:}" ] || { echo "dt: $r"; exit 1; }
    done
    fdtget $t /wdt-reboot compatible | grep -qx wdt-reboot
    cpld=$(fdtget -l $t /soc/io-bus@fc000000/i2c@1880000/i2c-mux@70/i2c@3 | grep -x cpld@68)
    [ "$(fdtget $t /soc/io-bus@fc000000/i2c@1880000/i2c-mux@70/i2c@3/$cpld compatible)" = edgecore,dmc12-cpld ]
    [ "$(fdtget -t x $t /soc/io-bus@fc000000/serial@1883000 clock-frequency)" = 1dcd6500 ]
    fdtget $t /soc/io-bus@fc000000/serial@1883000 bootph-all >/dev/null
    echo "dt: ok (4 GiB in 2 banks, 4 firmware ranges, CPLD + sp805 resets, UART0 500 MHz pre-reloc)"
    mkdir $out; echo pass > $out/result
  '';

  probe = pkgs.runCommand "alpine-v2-booti-probe" { nativeBuildInputs = [ binutils ]; } ''
    aarch64-unknown-linux-gnu-as -o probe.o ${./booti-probe.S}
    aarch64-unknown-linux-gnu-ld -Ttext=0 -o probe.elf probe.o
    mkdir $out; aarch64-unknown-linux-gnu-objcopy -O binary probe.elf $out/probe.bin
  '';

  # GPT disk, one ext4 partition: /boot/extlinux/extlinux.conf -> probe +
  # the QEMU control DT without /reserved-memory (stands in for a kernel-tree
  # DTB that knows nothing about the firmware).
  disk = pkgs.runCommand "alpine-v2-test-disk" {
    nativeBuildInputs = [ pkgs.dtc pkgs.e2fsprogs pkgs.util-linux ];
  } ''
    mkdir -p root/boot/extlinux
    cp ${probe}/probe.bin root/boot/probe.bin
    cp ${alpine.qemu-virt}/u-boot.dtb root/boot/test.dtb; chmod u+w root/boot/test.dtb
    fdtput -r root/boot/test.dtb /reserved-memory
    cat > root/boot/extlinux/extlinux.conf <<'EOF'
    DEFAULT probe
    TIMEOUT 1
    LABEL probe
      KERNEL /boot/probe.bin
      FDT /boot/test.dtb
      APPEND console=ttyAMA0
    EOF
    truncate -s 32M part.img
    mkfs.ext4 -q -L testboot -E root_owner=0:0 -d root part.img
    mkdir $out; truncate -s 40M $out/disk.img
    printf 'label: gpt\nstart=2048, size=65536, type=linux\n' | sfdisk -q $out/disk.img
    dd if=part.img of=$out/disk.img bs=512 seek=2048 conv=notrunc status=none
  '';

  qemu = pkgs.runCommand "alpine-v2-uboot-qemu" { nativeBuildInputs = [ pkgs.qemu pkgs.python3 ]; } ''
    set -euo pipefail
    cp ${disk}/disk.img .; chmod u+w disk.img
    python3 ${./qemu-test.py} ${pkgs.qemu}/bin/qemu-system-aarch64 \
      ${alpine.qemu-virt}/u-boot.bin disk.img | tee log.txt
    mkdir $out; cp log.txt $out/
  '';
in pkgs.linkFarm "alpine-v2-uboot-tests" [
  { name = "static"; path = static; }
  { name = "qemu-virt"; path = qemu; }
]
