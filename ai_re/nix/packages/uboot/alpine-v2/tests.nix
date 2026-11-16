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
#  selfinit: dmc12-selfinit (external PCIe init, MUIO/GPIO/CCU, SPI NOR
#          read-only except the env window): config, linked/absent code
#          (no lock/SR-write/quad-enable code, no sf write/erase/update/
#          protect), instruction order of the SSI and GPIO writes, DT
#          additions, IPXE slot fit, chainload u-boot.bin identity;
#          dmc12-stage3app: alboot.py check, the wrapped data is that
#          u-boot.bin byte for byte.
#  spi-window: ./window-test, the write-window range check and spi-mem
#          command filter (include/spi_flash_window.h): boundaries, every
#          opcode x address width x dummy x direction, random requests.
#  sandbox-window: the series on U-Boot sandbox with the emulated
#          MX25U12835F: erase/write/raw-op attempts inside and outside the
#          window; the backing file may change only inside it.
#  pcie-init: ./pcie-test, al_init_pcie_rc.c (from the series) on the
#          verbatim HAL (./al-pcie-hal) against a register model: cold,
#          warm (factory state) and stuck-link runs, factory AXI words
#          reproduced; and the HAL copy inside patch 0017 is verbatim.
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
             BOOTDEV_ETH NET_RANDOM_ETHADDR CMD_NFS CMD_TLV_EEPROM CMD_EEPROM \
             PCIE_ALPINE_V2 ALPINE_V2_MUIO_INIT ALPINE_V2_GPIO_INIT ALPINE_V2_CCU_IO_COHERENCY \
             CMD_ALPINE_V2 SPI_FLASH_WRITE_WINDOW ENV_REDUNDANT; do
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
  si = alpine.dmc12-selfinit;
  s3 = alpine.dmc12-stage3app;
  selfinit = pkgs.runCommand "alpine-v2-uboot-selfinit-checks" {
    nativeBuildInputs = [ binutils pkgs.python3 pkgs.gnugrep pkgs.dtc ];
  } ''
    set -euo pipefail
    c=${si}/.config
    for k in SPI_FLASH_LOCK SPI_FLASH_UNLOCK_ALL SPI_FLASH_SFDP_SUPPORT SPI_FLASH_SOFT_RESET \
             SPI_FLASH_BAR SPI_FLASH_MTD CMD_SF_TEST CMD_SPI CMD_MTD CMD_MTDPARTS DM_MTD \
             MTD_NOR_FLASH ENV_IS_NOWHERE ENV_SPI_EARLY ENV_SECT_SIZE_AUTO EFI_LOADER PSCI_RESET SYSRESET_PSCI \
             WATCHDOG_AUTOSTART MMC CMD_EXT4_WRITE FAT_WRITE BOOTDEV_ETH CMD_EEPROM \
             CMD_FDT LZMA CMD_MII CMD_ELF CMD_LOADS; do
      ! grep -q "^CONFIG_$k=y" $c || { echo "config: CONFIG_$k is on"; exit 1; }
    done
    # The chainload build is the one u01 ran in RAM and the disk image ships: its bytes
    # are pinned. Change these on purpose only (u-boot-port.md "what exists").
    sha256sum ${d}/u-boot.bin ${d}/u-boot.uimage | awk '{print $1}' | tr '\n' ' ' > chain.txt
    [ "$(cat chain.txt)" = "f4b4b4f3af25c8d85d6bc6585902b0ed4e8f8f5e6849fd109ca83d857a41e009 062428a9304388e877abbb70cfb66bf5cab4b11404148eb12f84a8e1d6824b52 " ] || \
      { echo "chainload: bytes changed: $(cat chain.txt)"; exit 1; }
    echo "chainload: ok (u-boot.bin f4b4b4f3, uimage 062428a9, unchanged)"
    # the stage3 app must fit the 512 KiB IPXE slot with >= 16 KiB to spare
    sz=$(stat -c %s ${s3}/u-boot-ipxe-08000000.fimg)
    [ "$sz" -le $((0x80000 - 0x4000)) ] || { echo "size: .fimg $sz B leaves < 16 KiB in the IPXE slot"; exit 1; }
    echo "size: ok (.fimg $sz B, $((0x80000 - sz)) B free in the IPXE slot)"
    for kv in TEXT_BASE=0x08000000 POSITION_INDEPENDENT=y \
              ENV_IS_IN_SPI_FLASH=y ENV_REDUNDANT=y ENV_OFFSET=0xf90000 ENV_OFFSET_REDUND=0xfa0000 \
              ENV_SIZE=0x10000 ENV_SECT_SIZE=0x10000 ENV_SPI_BUS=0 ENV_SPI_CS=0 \
              ENV_SPI_MAX_HZ=25000000 CMD_SAVEENV=y SPI_FLASH_WRITE_WINDOW=y \
              SPI_FLASH_WRITE_WINDOW_BUS=0 SPI_FLASH_WRITE_WINDOW_CS=0 \
              SPI_FLASH_WRITE_WINDOW_START=0xf90000 SPI_FLASH_WRITE_WINDOW_SIZE=0x20000 \
              SPI_FLASH_WRITE_WINDOW_JEDEC=0xc22538 SPI_FLASH_WRITE_WINDOW_CHIP_SIZE=0x1000000 \
              ENV_ADDR=0x0 \
              CMD_EXT4=y CMD_LOADB=y CMD_WGET=y CMD_I2C=y CMD_PCI=y BOOTMETH_EXTLINUX=y \
              'LOCALVERSION="-dmc12-selfinit"' PCIE_ALPINE_V2=y CMD_ALPINE_V2_PCIE=y PCI_INIT_R=y \
              ALPINE_V2_MUIO_INIT=y ALPINE_V2_GPIO_INIT=y ALPINE_V2_CCU_IO_COHERENCY=y CMD_ALPINE_V2=y \
              DESIGNWARE_SPI=y DM_SPI_FLASH=y SPI_FLASH_MACRONIX=y CMD_SF=y \
              SPI_FLASH_READ_ONLY=y SPI_MEM_READ_ONLY=y AL_ETH=y SYSRESET_DMC12_CPLD=y; do
      grep -qx "CONFIG_$kv" $c || { echo "config: CONFIG_$kv missing"; exit 1; }
    done
    # both env copies inside the window, sector aligned, clear of the factory env pair
    python3 - $c <<'PY'
    import sys
    c = dict(l.strip().split("=", 1) for l in open(sys.argv[1]) if l.startswith("CONFIG_") and "=" in l)
    h = lambda k: int(c["CONFIG_" + k], 0)
    ws, wz, sz, sect = h("SPI_FLASH_WRITE_WINDOW_START"), h("SPI_FLASH_WRITE_WINDOW_SIZE"), h("ENV_SIZE"), h("ENV_SECT_SIZE")
    for off in (h("ENV_OFFSET"), h("ENV_OFFSET_REDUND")):
        assert ws <= off and off + sect <= ws + wz and sz <= sect and off % sect == 0, hex(off)
        assert off + sect <= 0xfd0000 or off >= 0x1000000, f"{off:#x} hits the factory env"
    assert abs(h("ENV_OFFSET") - h("ENV_OFFSET_REDUND")) >= sect
    assert ws >= 0xb5f000 and ws + wz <= 0xfb0000, "window outside the blank BOOT_APP 1 tail"
    assert ws % 0x10000 == 0 and wz % 0x10000 == 0, "window not 64 KiB aligned"
    print(f"env: ok (copies {h('ENV_OFFSET'):#x}/{h('ENV_OFFSET_REDUND'):#x} x {sect:#x} inside window {ws:#x}+{wz:#x}; factory env 0xfd0000-0xfeffff untouched)")
    PY
    echo "config: ok (PCIe RC init, MUIO/CCU, read-only SF except the env window; no SF lock/unlock, env in SPI)"

    h=$(aarch64-unknown-linux-gnu-readelf -h ${si}/u-boot)
    grep -qi 'Entry point address:.*0x8000000$' <<<"$h"
    aarch64-unknown-linux-gnu-objdump -d ${si}/u-boot > dis.txt
    # the only smc/hvc-free rule still holds; eret only in the SError vector
    n=$(grep -cE '\s(smc|hvc)\s' dis.txt || true)
    [ "$n" = 0 ] || { echo "$n smc/hvc instructions"; exit 1; }
    aarch64-unknown-linux-gnu-nm ${si}/u-boot > syms.txt
    for s in _u_boot_list_2_driver_2_pcie_alpine_v2 _u_boot_list_2_driver_2_dw_spi \
             _u_boot_list_2_cmd_2_sf _u_boot_list_2_cmd_2_alpcie _u_boot_list_2_cmd_2_alpine \
             al_init_pcie_rc al_pcie_port_config alpine_v2_serror_drain alpine_v2_muio_init alpine_v2_gpio_init \
             alpine_v2_ccu_io_coherency spi_nor_ro_erase spi_nor_ro_write board_prep_linux; do
      grep -q " $s$" syms.txt || { echo "elf: no $s"; exit 1; }
    done
    # u01 run 2: U-Boot runs with SErrors unmasked and routed to EL2, so the
    # downstream config accessors must mask PSTATE.A around the access + drain,
    # and "alpcie reinit" must unbind the old children (bus numbering).
    fn() { awk -v f="<$1>:" '$2 == f {p=1; next} p && /^$/ {exit} p' dis.txt; }
    for f in alpine_pcie_read_config alpine_pcie_write_config; do
      fn $f | grep -q 'msr.daifset, #0x4' || { echo "elf: $f does not mask SError"; exit 1; }
      fn $f | grep -q 'bl.*<alpine_v2_serror_drain>' || { echo "elf: $f does not drain"; exit 1; }
    done
    fn do_alpcie | grep -q 'bl.*<device_chld_unbind>' || { echo "elf: reinit keeps stale children"; exit 1; }
    # board_init: MUIO pin functions, then the GPIO list (the vendor order)
    [ "$(fn board_init | grep -oE 'bl.*<alpine_v2_(muio|gpio)_init>' | sed 's/.*<//; s/>//' | tr '\n' ' ')" = \
      "alpine_v2_muio_init alpine_v2_gpio_init " ] || { echo "elf: board_init order"; exit 1; }
    # No lock/SR/quad-enable/SST code and no sf write paths. That erase/program
    # only happens inside the window is shown by sandbox-window, not here.
    for s in sst_write sst_write_byteprogram write_sr write_sr_and_check macronix_quad_enable \
             stm_lock stm_unlock spi_flash_update spi_flash_update_block do_spi_flash_erase \
             do_spi_protect do_spi_flash_test; do
      ! grep -q " $s$" syms.txt || { echo "elf: $s is linked"; exit 1; }
    done
    b=${si}/u-boot.bin
    grep -aqF 'SF: erase %#llx+%#llx outside the write window, refused' $b
    grep -aqF 'SF: write %#llx+%#zx outside the write window, refused' $b
    # cold stage3 boot: no slave left selected before the first command (0018);
    # the env origin is printed at boot and in "alpine state", never stored in the env (0021)
    grep -aq 'slave select %#x left set by the firmware, cleared' $b
    grep -aq 'Env:   SPI flash, copy at %#x' $b
    grep -aq 'Env:   compiled-in default' $b
    grep -q ' alpine_v2_env_report$' syms.txt
    fn board_late_init | grep -q 'bl.*<alpine_v2_env_report>' || { echo "elf: board_late_init does not report the env"; exit 1; }
    fn do_alpine | grep -q 'bl.*<alpine_v2_env_report>' || { echo "elf: alpine state does not report the env"; exit 1; }
    ! grep -aq 'env_source' $b || { echo "elf: env_source is back (saveenv would store it)"; exit 1; }
    # Instruction order (0018): probe stores SER (+0x10) = 0 before it calls the
    # variant init (blr); exec_op stores SER = 0 after the first SSIENR (+0x08) = 0 and
    # before the first DR (+0x60) write; GPIO init stores DATA before and after DIR (+0x400).
    for f in dw_spi_probe dw_spi_exec_op alpine_v2_gpio_init; do fn $f > $f.dis; done
    python3 - <<'PY'
    import re
    def ins(f):
        # "addr:\tbytes \tmnemonic\toperands" -> "mnemonic\toperands"
        return [l.split("\t", 2)[2] if l.count("\t") >= 2 else l for l in open(f + ".dis")]
    def first(lines, pat, after=-1):
        for i, l in enumerate(lines):
            if i > after and re.search(pat, l):
                return i
        raise SystemExit(f"elf: no /{pat}/ after {after}")
    p = ins("dw_spi_probe")
    blr = first(p, r"\bblr\b")
    off = first(p, r"str\s+wzr, \[x\d+, #8\]")
    ser = first(p, r"str\s+wzr, \[x\d+, #16\]", off)
    assert off < ser < blr, f"probe: SSIENR=0 {off}, SER=0 {ser}, variant init {blr}"
    # SER = 0 must sit inside the SSIENR = 0 ... SSIENR = 1 window that precedes the
    # FIFO load (the end-of-transfer SER = 0 can be laid out earlier in the code)
    e = ins("dw_spi_exec_op")
    off = first(e, r"str\s+wzr, \[x\d+, #8\]")
    on = first(e, r"str\s+w\d+, \[x\d+, #8\]", off)
    ser = first(e, r"str\s+wzr, \[x\d+, #16\]", off)
    dr = first(e, r"str\s+w\d+, \[x\d+, #96\]|bl\s+\S+ <dw_writer>", on)
    assert off < ser < on < dr, f"exec_op: SSIENR=0 {off}, SER=0 {ser}, SSIENR=1 {on}, DR {dr}"
    g = ins("alpine_v2_gpio_init")
    stores = [i for i, l in enumerate(g) if re.search(r"\bstr\s+w\d+, \[x\d+(, x\d+)?\]$", l.strip())]
    d = first(g, r"\bstr\s+w\d+, \[x\d+, #1024\]")
    assert any(i < d for i in stores) and any(i > d for i in stores), \
        f"gpio: DATA stores {stores} not on both sides of DIR at {d}"
    print("order: ok (SER cleared before the SSI variant init and before each transfer's FIFO load; GPIO value, direction, value)")
    PY
    ! grep -aqE 'sf (write|erase|update|protect) |destructive test' $b || { echo "sf write help present"; exit 1; }
    grep -aq 'read-only build: no write, erase, update or protect' $b
    grep -aq 'spi-mem: read-only build, opcode %02x refused' $b
    echo "elf: ok (PCIe/SPI drivers, alpcie/alpine/sf commands; SPI NOR erase/program only behind the window hooks; no lock, SR-write or quad-enable code; sf read-only)"

    t=${si}/u-boot.dtb
    [ "$(fdtget -t x $t /muio@fd8a8000 amazon,pins)" = "0 2 1 2 14 2 15 2 16 0" ]
    # GPIO 22 = bank 2 line 6, out, high (dt.0 board-cfg gpio-list <0x16 1 1>)
    [ "$(fdtget $t /gpio-init compatible)" = amazon,alpine-v2-gpio-init ]
    [ "$(fdtget $t /gpio-init amazon,gpio-list)" = "22 1 1" ]
    i=0
    for ph in $(fdtget $t /gpio-init gpio-controllers); do
      n=$(fdtget -l $t /soc/io-bus@fc000000 | while read c; do
            if [ "$(fdtget $t /soc/io-bus@fc000000/$c phandle 2>/dev/null || true)" = "$ph" ]; then
              echo $c; fi; done)
      want=$(printf 'gpio@%x' $(( i < 5 ? 0x1887000 + i * 0x1000 : 0x1897000 )))
      [ "$n" = "$want" ] || { echo "dt: gpio bank $i is '$n', want $want"; exit 1; }
      fdtget $t /soc/io-bus@fc000000/$n compatible | grep -q arm,pl061
      i=$((i + 1))
    done
    [ $i = 6 ] || { echo "dt: $i gpio banks"; exit 1; }
    [ "$(fdtget $t /muio@fd8a8000 compatible)" = amazon,alpine-v2-muio ]
    p=/pcie@fb600000
    [ "$(fdtget $t $p compatible)" = amazon,al-alpine-v2-pcie ]
    [ "$(fdtget $t $p reg-names)" = "config controller dbi" ]
    [ "$(fdtget $t $p amazon,link-gen) $(fdtget $t $p amazon,link-lanes) $(fdtget $t $p amazon,port-id)" = "1 1 0" ]
    sp=/soc/io-bus@fc000000/spi@1882000
    [ "$(fdtget $t /aliases spi0)" = $sp ] || { echo "dt: spi0 alias"; exit 1; }
    fdtget $t $sp compatible | grep -q amazon,alpine-dw-apb-ssi
    [ "$(fdtget $t $sp/flash@0 spi-max-frequency) $(fdtget $t $sp/flash@1 spi-max-frequency)" = "25000000 25000000" ]
    # the controller caps every transfer (500 kHz default: 16 MiB took 260 s on u01)
    [ "$(fdtget $t $sp spi-max-frequency)" = 25000000 ]
    # the chainload DTB has none of it
    ! fdtget -l ${d}/u-boot.dtb / | grep -qx muio@fd8a8000
    ! fdtget ${d}/u-boot.dtb $p amazon,link-gen 2>/dev/null
    ! fdtget -l ${d}/u-boot.dtb / | grep -qx gpio-init
    echo "dt: ok (MUIO pins 0,1,20,21 fn 2, 22 fn 0; GPIO 22 out 1 on PL061 0xfd889000 line 6; PCIe port 0 gen1 x1; SPI NOR CS0/CS1 and the SSI at 25 MHz; chainload DT unchanged)"

    python3 - ${si}/u-boot.bin ${s3} <<'PY'
    import struct, sys, glob
    b = open(sys.argv[1], "rb").read()
    for f in sorted(glob.glob(sys.argv[2] + "/*.fimg")):
        o = open(f, "rb").read()
        magic, _, oid = struct.unpack_from("<3I", o)
        size, load, ex = struct.unpack_from("<3Q", o, 0x28)
        assert magic == 0x000B9EC7 and oid == 0x110, f
        assert o[0x48:0x48 + size] == b and size == len(b), f + ": data is not u-boot.bin"
        assert len(o) <= 0x80000, f"{f}: {len(o):#x} B > IPXE slot 0x80000"
        assert load == ex and load in (0x08000000, 0x01100000), f
        print(f"stage3app: ok ({f.split('/')[-1]}: {len(o):#x} B, {0x80000 - len(o)} B left in the IPXE slot, load = exec = {load:#x})")
    PY
    grep -c ': OK$' ${s3}/check.txt | grep -qx 2
    mkdir $out; echo pass > $out/result
  '';

  # include/spi_flash_window.h out of the series: the SPI NOR write window's
  # range check and command filter, on the host with the DMC-12 window.
  windowTest = pkgs.runCommandCC "alpine-v2-uboot-spi-window" {
    nativeBuildInputs = [ pkgs.patchutils ];
    test = builtins.path { path = ./window-test; name = "alpine-v2-window-test"; };
  } ''
    set -euo pipefail
    for p in ${toString alpine.patches}; do
      filterdiff -i '*/include/spi_flash_window.h' "$p"
    done > w.diff
    mkdir ub; (cd ub && patch -s -p1 < ../w.diff)
    $CC -O2 -Wall -Werror -Iub/include -o run $test/window-test.c
    ./run | tee log.txt
    mkdir $out; cp log.txt $out/
  '';

  # The series on U-Boot sandbox (native), read-only SPI NOR + the DMC-12 write
  # window, the sandbox flash emulating an MX25U12835F, plus a test-only command
  # (./sandbox-window/sfwtest.c, never in a board build) that drives the flash
  # like code would. ./sandbox-window/run.py checks the results and that the
  # backing file changes only inside the window.
  sandboxUBoot = (pkgs.buildUBoot {
    defconfig = "sandbox_defconfig";
    extraConfig = builtins.readFile ./sandbox-window/sandbox.config;
    extraPatches = alpine.patches;
    extraMeta.platforms = [ "x86_64-linux" ];
    filesToInstall = [ "u-boot" "u-boot.dtb" ".config" ];
  }).overrideAttrs (o: {
    pname = "uboot-sandbox-spi-window";
    postPatch = o.postPatch + alpine.reproducibleVersion + ''
      cp ${./sandbox-window/sfwtest.c} cmd/sfwtest.c
      echo 'obj-$(CONFIG_SANDBOX) += sfwtest.o' >> cmd/Makefile
      substituteInPlace arch/sandbox/dts/sandbox.dtsi \
        --replace-fail '"spansion,m25p16", "jedec,spi-nor"' '"macronix,mx25u12835f", "jedec,spi-nor"'
    '';
  });
  sandboxWindow = pkgs.runCommand "alpine-v2-uboot-sandbox-window" {
    nativeBuildInputs = [ pkgs.python3 ];
  } ''
    set -euo pipefail
    grep -qx CONFIG_SPI_FLASH_WRITE_WINDOW=y ${sandboxUBoot}/.config
    python3 ${./sandbox-window/run.py} ${sandboxUBoot}/u-boot ${sandboxUBoot}/u-boot.dtb | tee log.txt
    mkdir $out; cp log.txt $out/
  '';

  pcieInit = pkgs.runCommandCC "alpine-v2-uboot-pcie-init" {
    nativeBuildInputs = [ pkgs.patchutils ];
    hal = builtins.path { path = ./al-pcie-hal; name = "al-pcie-hal"; };
    test = builtins.path { path = ./pcie-test; name = "alpine-v2-pcie-test"; };
  } ''
    set -euo pipefail
    for p in ${toString alpine.patches}; do
      filterdiff -i '*/drivers/pci/al_pcie/*' "$p"
    done > al.diff
    mkdir ub; (cd ub && patch -s -p1 < ../al.diff)
    # u01 2026-11-15: an int-returning DT read of a 0xfd8xxxxx base sign-extended
    # into the PBS pointer (abort at probe). No register base may come that way.
    for p in ${toString alpine.patches}; do
      filterdiff -i '*/drivers/pci/pcie_alpine_v2.c' -i '*/board/amazon/alpine-v2/*' "$p"
    done > glue.diff
    ! grep -nE '^\+.*\*\)\s*\(u?intptr_t\)\s*dev_read_u32_default' glue.diff || \
      { echo "glue: register base through dev_read_u32_default (int)"; exit 1; }
    a=ub/drivers/pci/al_pcie
    n=0
    for f in $a/al_hal_*; do
      b=$(basename $f)
      case $b in al_hal_plat_services.h|al_hal_plat_types.h) continue;; esac
      cmp -s $f $hal/$b || { echo "hal: $b differs from al-pcie-hal/"; exit 1; }
      n=$((n + 1))
    done
    [ $n = $(ls $hal | grep -c '^al_hal_') ] || { echo "hal: $n files in the patch"; exit 1; }
    mkdir t; cp $hal/* $test/*.h $a/al_init_pcie_rc.[ch] t/
    $CC -O1 -Wall -Werror -Wno-unused-function -Wno-unused-variable -Wno-unused-but-set-variable \
      -Wno-address-of-packed-member -It \
      -DAL_DEV_ID_ALPINE_V1=0 -DAL_DEV_ID_ALPINE_V2=1 -DAL_DEV_ID_ALPINE_V3=2 \
      -DAL_DEV_ID_ALPINE_V4=3 -DAL_DEV_ID=AL_DEV_ID_ALPINE_V2 -o run $test/pcie-init-test.c \
      t/al_init_pcie_rc.c t/al_hal_pcie.c t/al_hal_pbs_utils.c \
      t/al_hal_pcie_reg_ptr_set_rev2.c t/al_hal_pcie_reg_ptr_set_rev3.c
    ./run | tee log.txt
    echo "hal: ok ($n files in patch 0017 identical to al-pcie-hal/)" | tee -a log.txt
    mkdir $out; cp log.txt $out/
  '';
in pkgs.linkFarm "alpine-v2-uboot-tests" [
  { name = "static"; path = static; }
  { name = "eth-params"; path = ethParams; }
  { name = "qemu-virt"; path = qemu; }
  { name = "selfinit"; path = selfinit; }
  { name = "pcie-init"; path = pcieInit; }
  { name = "spi-window"; path = windowTest; }
  { name = "sandbox-window"; path = sandboxWindow; }
]
