{ pkgs, alpine }:
# Checks for the Alpine V3 / CS8210 U-Boot that need no board:
#  static: AArch64 ELF linked at the load address, Linux Image header
#          (text_offset = load address, place anywhere: the vendor booti runs
#          it in place), uImage header, RAM footprint below the kernel's
#          0x10000000 FDT slot, no HVC; RAM-only/read-only config (no env
#          storage, no SPI NOR write/lock code, PSCI reset, no network/EFI);
#          the CS8210 control DT (4 DRAM banks, firmware ranges, UART0 500 MHz
#          pre-relocation, the 7 unused AHCI functions disabled, boot SSI).
#  qemu:   ./qemu-test.py on QEMU virt with the same board code and config:
#          booti-style and MMU-on "go" entries, the CS8210 NixOS boot.scr ->
#          bootmenu -> booti chain from an MBR/ext4 disk, memreserve copy,
#          PSCI reset.
let
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  binutils = cross.buildPackages.binutils-unwrapped;
  d = alpine.cs8210;
  static = pkgs.runCommand "alpine-v3-uboot-static-checks" {
    nativeBuildInputs = [ binutils pkgs.python3 pkgs.gnugrep pkgs.dtc ];
  } ''
    set -euo pipefail
    h=$(aarch64-unknown-linux-gnu-readelf -h ${d}/u-boot)
    grep -q 'Class:.*ELF64' <<<"$h"; grep -q 'Machine:.*AArch64' <<<"$h"
    grep -qi 'Entry point address:.*0x8000000$' <<<"$h" || { grep Entry <<<"$h"; exit 1; }
    aarch64-unknown-linux-gnu-objdump -d ${d}/u-boot > dis.txt
    # smc/hvc only in the SMCCC helpers (PSCI SYSTEM_RESET); the DT picks smc
    python3 - dis.txt <<'PY'
    import re, sys
    f, bad, seen = None, [], set()
    for l in open(sys.argv[1]):
        m = re.match(r"^[0-9a-f]+ <(.*)>:$", l)
        if m:
            f = m.group(1)
            continue
        if re.search(r"\t(smc|hvc)\t", l):
            seen.add(f)
            if f not in ("__arm_smccc_smc", "__arm_smccc_hvc", "arm_smccc_1_2_smc"):
                bad.append((f, l.strip()))
    assert not bad, bad
    print("elf: ok (AArch64, entry 0x08000000, smc/hvc only in " + " ".join(sorted(seen)) + ")")
    PY

    python3 - ${d}/u-boot.bin ${alpine.loadAddr} <<'PY'
    import struct, sys
    b = open(sys.argv[1], "rb").read(); load = int(sys.argv[2], 16)
    assert b[0x38:0x3c] == b"ARM\x64", "no Linux Image magic"
    text_off, image_size, flags = struct.unpack_from("<QQQ", b, 8)
    assert text_off == load, hex(text_off)
    assert flags & 0b1010 == 0b1010, hex(flags)   # 4K pages, place anywhere
    end = load + image_size
    # the CS8210 bootmenu puts the FDT at 0x10000000 (we are dead by then,
    # but the vendor booti must not see an overlap)
    assert end < 0x10000000 and image_size < 0x200000, hex(end)
    print(f"image: ok ({len(b)} B file, {image_size:#x} B in RAM, {load:#x}-{end:#x})")
    PY

    u=${d}/u-boot.uimage.txt; cat $u
    grep -q 'Image Type:.*AArch64 Linux Kernel Image (uncompressed)' $u
    grep -q 'Load Address: 08000000' $u; grep -q 'Entry Point:  08000000' $u
    echo "uimage: ok"

    c=${d}/.config
    for k in CMD_SAVEENV EFI_LOADER PSCI_RESET ENV_IS_IN_SPI_FLASH SPI_FLASH_WRITE_WINDOW \
             SPI_FLASH_LOCK SPI_FLASH_UNLOCK_ALL SPI_FLASH_SFDP_SUPPORT SPI_FLASH_SOFT_RESET \
             SPI_FLASH_BAR SPI_FLASH_MTD CMD_SF_TEST CMD_SPI CMD_MTD DM_MTD MMC NET \
             CMD_EXT4_WRITE FAT_WRITE CMD_EEPROM CMD_TLV_EEPROM ARMV8_MULTIENTRY ARMV8_PSCI GICV3 \
             WATCHDOG_AUTOSTART; do
      ! grep -q "^CONFIG_$k=y" $c || { echo "config: CONFIG_$k is on"; exit 1; }
    done
    for kv in TEXT_BASE=0x08000000 POSITION_INDEPENDENT=y ENV_IS_NOWHERE=y NR_DRAM_BANKS=4 \
              SPI_FLASH_READ_ONLY=y SPI_MEM_READ_ONLY=y SPI_MEM_READ_ONLY_4B=y DESIGNWARE_SPI=y \
              SPI_FLASH_MACRONIX=y CMD_SF=y SYSRESET_PSCI=y ARM_PSCI_FW=y \
              PCI_INIT_R=y PCIE_ECAM_GENERIC=y AHCI_PCI=y SCSI=y CMD_EXT4=y CMD_FAT=y \
              CMD_BOOTI=y CMD_BOOTMENU=y CMD_IMPORTENV=y CMD_SOURCE=y LEGACY_IMAGE_FORMAT=y \
              BOOTMETH_SCRIPT=y BOOTMETH_EXTLINUX=y CMD_LOADB=y OF_BOARD_SETUP=y \
              SYS_I2C_DW=y I2C_MUX_PCA954x=y \
              'DEFAULT_FDT_FILE="amazon/alpine-v3-cs8210.dtb"' 'BOOTCOMMAND="bootflow scan -lb"'; do
      grep -qx "CONFIG_$kv" $c || { echo "config: CONFIG_$kv missing"; exit 1; }
    done
    echo "config: ok (RAM env, SPI NOR read-only incl. 4-byte reads, PSCI reset, no network/EFI/MMC)"
    aarch64-unknown-linux-gnu-nm ${d}/u-boot > syms.txt
    for s in spi_nor_ro_erase spi_nor_ro_write save_boot_params ft_board_setup \
             _u_boot_list_2_driver_2_dw_spi _u_boot_list_2_driver_2_ahci_pci \
             _u_boot_list_2_cmd_2_sf _u_boot_list_2_cmd_2_bootmenu; do
      grep -q " $s$" syms.txt || { echo "elf: no $s"; exit 1; }
    done
    for s in spi_flash_update do_spi_flash_erase write_sr macronix_quad_enable stm_lock; do
      ! grep -q " $s$" syms.txt || { echo "elf: $s is linked"; exit 1; }
    done
    grep -aq 'read-only build: no write, erase, update or protect' ${d}/u-boot.bin
    echo "elf: ok (read-only SF, AHCI, bootmenu; no SPI NOR write/lock code)"

    t=${d}/u-boot.dtb
    [ "$(fdtget -t x $t /memory@0 reg)" = "0 0 0 80000000 0 80000000 0 40000000 1 0 1 0 2 0 0 40000000" ]
    for r in "secmon@0:0 0 0 100000" "ecc-poison-ch0@1e0000:0 1e0000 0 1000" \
             "ecc-poison-ch1@1e1100:0 1e1100 0 1000" "apcea@b0000000:0 b0000000 0 10000000"; do
      [ "$(fdtget -t x $t /reserved-memory/''${r%%:*} reg)" = "''${r#*:}" ] || { echo "dt: $r"; exit 1; }
    done
    u0=/soc/io-bus@fc000000/serial@1883000
    [ "$(fdtget -t x $t $u0 clock-frequency)" = 1dcd6500 ]
    fdtget $t $u0 bootph-all >/dev/null
    [ "$(fdtget $t /chosen stdout-path)" = serial0:115200n8 ]
    [ "$(fdtget $t /psci compatible) $(fdtget $t /psci method)" = "arm,psci-0.2 smc" ]
    [ "$(fdtget $t /timer clock-frequency)" = 75000000 ]
    p=/soc/pcie@fbd00000
    for s in 8 9 a c d e f; do
      [ "$(fdtget $t $p/sata@$s,0 status)" = disabled ] || { echo "dt: sata@$s,0"; exit 1; }
    done
    ! fdtget -l $t $p | grep -qx 'sata@b,0' || { echo "dt: the SSD's AHCI 00:0b.0 is disabled"; exit 1; }
    sp=/soc/io-bus@fc000000/spi@1882000
    [ "$(fdtget $t /aliases spi0)" = $sp ]
    fdtget $t $sp compatible | grep -q amazon,alpine-dw-apb-ssi
    [ "$(fdtget $t $sp spi-max-frequency) $(fdtget $t $sp/flash@0 spi-max-frequency)" = "25000000 25000000" ]
    echo "dt: ok (4 banks, 4 firmware ranges, UART0 500 MHz pre-reloc, PSCI, 7 AHCI functions off, boot SSI 25 MHz)"
    mkdir $out; echo pass > $out/result
  '';

  probe = pkgs.runCommand "alpine-v3-booti-probe" { nativeBuildInputs = [ binutils ]; } ''
    aarch64-unknown-linux-gnu-as -o probe.o ${../alpine-v2/booti-probe.S}
    aarch64-unknown-linux-gnu-ld -Ttext=0 -o probe.elf probe.o
    mkdir $out; aarch64-unknown-linux-gnu-objcopy -O binary probe.elf $out/probe.bin
  '';

  # The CS8210 SSD layout (MBR: p1 FAT 16 MiB, p2 ext4 bootable) with what
  # nepeat/nix's cs8210 uboot-menu module installs: /boot/boot.scr (the same
  # launcher commands; addresses moved into QEMU's RAM) -> /boot/u-boot.menu
  # (env import -t, one bootmenu entry: ext4load Image + initrd uImage + DTB,
  # booti). The OS DTB is the QEMU control DT without fw@7c000000.
  disk = pkgs.runCommand "alpine-v3-test-disk" {
    nativeBuildInputs = [ pkgs.dtc pkgs.e2fsprogs pkgs.util-linux pkgs.ubootTools pkgs.dosfstools ];
  } ''
    mkdir -p root/boot/nixos
    cp ${probe}/probe.bin root/boot/nixos/probe.bin
    cp ${alpine.qemu-virt}/u-boot.dtb root/boot/nixos/test.dtb; chmod u+w root/boot/nixos/test.dtb
    fdtput -r root/boot/nixos/test.dtb /reserved-memory/fw@7c000000
    head -c 4096 /dev/urandom > initrd.cpio
    mkimage -A arm64 -O linux -T ramdisk -C none -n "CS8210 NixOS initrd" \
      -d initrd.cpio root/boot/nixos/initrd.uImage > /dev/null
    cat > boot.cmd <<'EOF'
    echo "== CS8210 NixOS bootmenu =="
    scsi init
    if ext4load scsi 0:2 0x47100000 /boot/u-boot.menu; then
      env import -t 0x47100000 ''${filesize}
      bootmenu
    else
      echo "[boot.scr] /boot/u-boot.menu missing on scsi 0:2"
    fi
    EOF
    mkimage -A arm64 -O linux -T script -C none -a 0 -e 0 \
      -n "CS8210 NixOS bootmenu launcher" -d boot.cmd root/boot/boot.scr > /dev/null
    {
      echo "# Auto-generated by NixOS cs8210-uboot-menu -- do not edit."
      echo "bootmenu_delay=1"
      printf '%s' "bootmenu_0=NixOS - Default (test)=ext4load scsi 0:2 0x48000000 /boot/nixos/probe.bin"
      printf '%s' "; ext4load scsi 0:2 0x60000000 /boot/nixos/initrd.uImage"
      printf '%s' "; ext4load scsi 0:2 0x50000000 /boot/nixos/test.dtb"
      printf '%s' "; setenv bootargs 'init=/test console=ttyAMA0'"
      printf '%s\n' "; booti 0x48000000 0x60000000 0x50000000"
    } > root/boot/u-boot.menu
    truncate -s 64M p2.img
    mkfs.ext4 -q -L NIXOS_SD -E root_owner=0:0 -d root p2.img
    truncate -s 16M p1.img; mkfs.vfat -n FIRMWARE p1.img > /dev/null
    mkdir $out; truncate -s 100M $out/disk.img
    printf 'label: dos\nstart=16384, size=32768, type=b\nstart=49152, size=131072, type=83, bootable\n' | sfdisk -q $out/disk.img
    dd if=p1.img of=$out/disk.img bs=512 seek=16384 conv=notrunc status=none
    dd if=p2.img of=$out/disk.img bs=512 seek=49152 conv=notrunc status=none
  '';

  qemu = pkgs.runCommand "alpine-v3-uboot-qemu" { nativeBuildInputs = [ pkgs.qemu pkgs.python3 ]; } ''
    set -euo pipefail
    cp ${disk}/disk.img .; chmod u+w disk.img
    python3 ${./qemu-test.py} ${pkgs.qemu}/bin/qemu-system-aarch64 \
      ${alpine.qemu-virt}/u-boot.bin disk.img | tee log.txt
    mkdir $out; cp log.txt $out/
  '';
in pkgs.linkFarm "alpine-v3-uboot-tests" [
  { name = "static"; path = static; }
  { name = "qemu-virt"; path = qemu; }
]
