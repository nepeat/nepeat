{ pkgs, alpine }:
# Checks for the Alpine V2 chainload U-Boot that need no board:
#  static: AArch64 ELF linked at the load address, Linux Image header,
#          uImage header (kernel type, load = entry = loadAddr), footprint
#          clear of the vendor initrd staging, no SMC/HVC anywhere (the
#          firmware's PSCI v0.1 has no SYSTEM_RESET), RAM-only/safety config,
#          DMC-12 control DT contents (memory, firmware ranges, resets,
#          the al_eth port node).
#  eth-params: ./eth-test, the al_eth board-params/MAC handoff encoding of
#          include/alpine_v2_eth.h (from the patch series) against the
#          Annapurna HAL in ../../al-eth/src, compiled for the host; and
#          the HAL copy inside the driver patch is verbatim.
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
    for k in SPI_FLASH CMD_SF DM_SPI CMD_SAVEENV EFI_LOADER PSCI_RESET SYSRESET_PSCI \
             WATCHDOG_AUTOSTART MMC CMD_EXT4_WRITE FAT_WRITE CMD_MTD ENV_IS_IN_SPI_FLASH \
             ARMV8_MULTIENTRY ARMV8_PSCI GICV3 ARM_ERRATA_828024 ARMV8_SET_SMPEN \
             BOOTDEV_ETH NET_RANDOM_ETHADDR CMD_NFS CMD_TLV_EEPROM CMD_EEPROM; do
      ! grep -q "^CONFIG_$k=y" $c || { echo "config: CONFIG_$k is on"; exit 1; }
    done
    for kv in TEXT_BASE=0x08000000 POSITION_INDEPENDENT=y ENV_IS_NOWHERE=y SYSRESET_DMC12_CPLD=y \
              ALPINE_V2_ETH_HANDOFF=y CMD_ALPINE_V2_ETH=y I2C_EEPROM=y \
              NET=y NET_LEGACY=y AL_ETH=y CMD_DHCP=y CMD_TFTPBOOT=y CMD_PING=y CMD_WGET=y \
              PCI_INIT_R=y \
              SYSRESET_WATCHDOG=y WDT_SP805=y CMD_BOOTI=y CMD_SYSBOOT=y BOOTMETH_EXTLINUX=y \
              PCIE_ECAM_GENERIC=y AHCI_PCI=y SCSI=y CMD_EXT4=y CMD_FAT=y OF_BOARD_SETUP=y \
              NR_DRAM_BANKS=2 'DEFAULT_FDT_FILE="amazon/alpine-v2-dmc12.dtb"' \
              'BOOTCOMMAND="bootflow scan -lb"'; do
      grep -qx "CONFIG_$kv" $c || { echo "config: CONFIG_$kv missing"; exit 1; }
    done
    echo "config: ok (RAM only: no SPI flash, no env storage, no EEPROM writer, no PSCI reset; al_eth net, no ethernet bootdev)"
    aarch64-unknown-linux-gnu-nm ${d}/u-boot > syms.txt
    for s in _u_boot_list_2_driver_2_al_eth board_prep_linux al_eth_flr_rmn; do
      grep -q " $s$" syms.txt || { echo "elf: no $s"; exit 1; }
    done
    echo "elf: ok (al_eth driver, OS handoff hook, FLR path linked)"

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
    e=/soc/pci@fbc00000/ethernet@1,0
    [ "$(fdtget -t x $t $e reg)" = "800 0 0 0 0" ]
    [ "$(fdtget $t $e phy-mode)" = rgmii ]
    [ "$(fdtget $t $e/fixed-link speed)" = 1000 ]
    fdtget $t $e/fixed-link full-duplex >/dev/null
    ee=/soc/io-bus@fc000000/i2c@1880000/i2c-mux@70/i2c@3/i2c-mux@71/i2c@2/eeprom@51
    [ "$(fdtget $t $e amazon,mac-eeprom)" = "$(fdtget $t $ee phandle)" ]
    [ "$(fdtget $t $ee compatible)" = atmel,24c02 ]
    echo "dt: ok (4 GiB in 2 banks, 4 firmware ranges, CPLD + sp805 resets, UART0 500 MHz pre-reloc, al_eth 00:01.0 rgmii fixed 1000 + EEPROM MAC)"
    mkdir $out; echo pass > $out/result
  '';

  # Host build: HAL headers + extracted HAL functions, host shims for the
  # two platform headers, include/alpine_v2_eth.h out of the series.
  ethParams = pkgs.runCommandCC "alpine-v2-uboot-eth-params" {
    nativeBuildInputs = [ pkgs.patchutils pkgs.gawk ];
    buildInputs = [ pkgs.zlib ];
    hal = builtins.path { path = ../../al-eth/src; name = "al_eth-src"; };
    test = builtins.path { path = ./eth-test; name = "alpine-v2-eth-test"; };
  } ''
    set -euo pipefail
    mkdir hal
    cp $hal/*.h hal/; chmod u+w hal/*
    cp $test/al_hal_plat_types.h $test/al_hal_plat_services.h hal/
    awk '/^\/\* board params register 1 \*\//{p=1} p{print} p&&/^int al_eth_board_params_get/{g=1} g&&/^}/{exit}' \
      $hal/al_hal_eth_main.c > hal/params_extract.c
    awk '/^int al_eth_mac_addr_store\(/{p=1} /^int al_eth_fwd_mhash_table_set/{exit} p{print}' \
      $hal/al_hal_eth_main.c > hal/mac_extract.c
    grep -q '^int al_eth_board_params_get' hal/params_extract.c
    grep -q '^int al_eth_mac_addr_read' hal/mac_extract.c
    for p in ${toString alpine.patches}; do
      filterdiff -i '*/include/alpine_v2_eth.h' "$p"
    done > header.diff
    mkdir ub; (cd ub && patch -s -p1 < ../header.diff)
    cp ub/include/alpine_v2_eth.h hal/

    # The HAL in the U-Boot patch is the Linux driver's, byte for byte,
    # apart from the two platform shims.
    for p in ${toString alpine.patches}; do
      filterdiff -i '*/drivers/net/al_eth/al_hal_*' "$p"
    done > hal.diff
    mkdir uh; (cd uh && patch -s -p1 < ../hal.diff)
    n=0
    for f in uh/drivers/net/al_eth/al_hal_*; do
      b=$(basename $f)
      case $b in al_hal_plat_services.h|al_hal_plat_types.h) continue;; esac
      cmp -s $f $hal/$b || { echo "hal: $b differs from packages/al-eth/src"; exit 1; }
      n=$((n + 1))
    done
    [ $n -ge 20 ] || { echo "hal: only $n files"; exit 1; }
    echo "hal: ok ($n files in the U-Boot patch identical to packages/al-eth/src)"
    $CC -O1 -Wall -Werror -Wno-unused-function -Ihal -o t $test/eth-params-test.c -lz
    ./t | tee log.txt
    echo "hal: ok ($n files verbatim)" >> log.txt
    mkdir $out; cp log.txt $out/
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
  { name = "eth-params"; path = ethParams; }
  { name = "qemu-virt"; path = qemu; }
]
