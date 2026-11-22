{ pkgs, uboots }:
# "Does this U-Boot run on the T2080's PowerPC core?" checks that need no board:
#  static: per image: ELF32 big-endian PowerPC, expected entry, built for
#          -mcpu=e6500, no SPE (e500v2-only) opcodes; per board (AS7760,
#          AS7772) the same checks for its NOR and chainload image: safety
#          config, NOR size/reset vector/layout as the factory one, and the
#          sha256 of each NOR u-boot.bin (flashed images: pinned).
#  qemu:   boot the same tree (qemu-ppce500) on an emulated e6500 core and
#          drive the shell (version, bdinfo, memory read/write/compare).
let
  binutils = pkgs.pkgsCross.ppc32.buildPackages.binutils-unwrapped;
  static = pkgs.runCommand "t208x-uboot-static-checks" {
    nativeBuildInputs = [ binutils pkgs.python3 pkgs.gnugrep ];
  } ''
    set -euo pipefail
    elf() { # name dir entry
      local n=$1 d=$2 entry=$3
      echo "== $n"
      h=$(powerpc-unknown-linux-gnu-readelf -h $d/u-boot)
      grep -q 'Class:.*ELF32' <<<"$h"; grep -q "big endian" <<<"$h"; grep -q 'Machine:.*PowerPC' <<<"$h"
      grep -qi "Entry point address:.*$entry" <<<"$h" || { echo "$n: entry not $entry"; grep Entry <<<"$h"; exit 1; }
      grep -qx 'CONFIG_E6500=y' $d/.config; grep -qx 'CONFIG_ARCH_T2080=y' $d/.config
      powerpc-unknown-linux-gnu-objdump -d -M e6500 $d/u-boot > dis.txt
      spe=$(grep -cE '\s(ev[a-z]+|efs[a-z]+|efd[a-z]+|brinc)\s' dis.txt || true)
      [ "$spe" = 0 ] || { echo "$n: $spe SPE (e500v2-only) instructions"; exit 1; }
      echo "$n: ok (entry $entry, e6500, $(grep -c . dis.txt) disassembly lines, 0 SPE)"
    }
    on()  { local n=$1 c=$2/.config kv; shift 2; for kv; do grep -qx "CONFIG_$kv" $c || { echo "$n: CONFIG_$kv missing"; exit 1; }; done; }
    off() { local n=$1 c=$2/.config k; shift 2; for k; do ! grep -q "^CONFIG_$k=y" $c || { echo "$n: CONFIG_$k is on"; exit 1; }; done; }
    # NOR image: exactly the factory partition size, ends in the reset-vector
    # branch; the ELF entry is the boot page (the vector at 0xeffffffc jumps there).
    nor() { # name dir
      elf $1 $2 0xeffff000
      python3 - $1 $2/u-boot.bin <<'PY'
    import sys
    b = open(sys.argv[2], "rb").read()
    assert len(b) == 786432, len(b)
    assert b[-4:] == bytes.fromhex("4bfff004"), b[-4:].hex()
    print(sys.argv[1] + ": 768 KiB, reset vector b .-0xffc")
    PY
    }
    # Every image of an AS7760-family board: nothing touches hardware the
    # board does not have (VID regulator, RDB CPLD, USB), its clock and DDR setup.
    board() { # name dir
      off $1 $2 VID MISC_INIT_R USB
      on $1 $2 SYS_CLK_FREQ=100000000 AS7760_BOARD=y AS7760_DDR=y DIMM_SLOTS_PER_CTLR=2
      grep -q 'bootflow scan -lb' $2/.config
      ! powerpc-unknown-linux-gnu-nm $2/u-boot | grep -qw adjust_vdd
    }
    # NOR replacement image: same layout as the factory U-Boot partition.
    board_nor() { # name dir
      nor $1 $2; board $1 $2
      off $1 $2 CHAINLOAD_CCSR_RELOCATED CMD_SPLTEST
      on $1 $2 TEXT_BASE=0xEFF40000 SYS_MONITOR_LEN=786432 ENV_IS_IN_FLASH=y ENV_ADDR=0xEFF20000 \
        ENV_SIZE=0x2000 ENV_SECT_SIZE=0x20000 SYS_QE_FMAN_FW_IN_NOR=y SYS_FMAN_FW_ADDR=0xEFF00000 PCI=y
      echo "$1: config ok"
    }
    # RAM second stage, started with "go" by the factory U-Boot.
    board_chainload() { # name dir
      elf $1 $2 0x200000; board $1 $2
      on $1 $2 CHAINLOAD_CCSR_RELOCATED=y BOOTDELAY=2 SYS_QE_FMAN_FW_IN_NOR=y SYS_FMAN_FW_ADDR=0xEFF00000
      echo "$1: safety config ok"
    }
    # A NOR image is flashed on a board, so its bytes are pinned: a nixpkgs
    # bump (compiler), a patch or a Kconfig edit that changes one fails here
    # and has to be acknowledged in this line. A refactor must not touch them.
    pin() { # name dir sha256
      [ "$(sha256sum < $2/u-boot.bin | cut -d' ' -f1)" = "$3" ] || \
        { echo "$1: u-boot.bin is $(sha256sum < $2/u-boot.bin | cut -d' ' -f1), pinned $3 (tests.nix)"; exit 1; }
      echo "$1: u-boot.bin $3, unchanged"
    }

    nor t2080rdb-nor ${uboots.t2080rdb-nor}

    board_nor as7760-nor ${uboots.as7760-nor}
    board_chainload as7760-chainload ${uboots.as7760-chainload}
    for d in ${uboots.as7760-nor} ${uboots.as7760-chainload}; do
      on as7760 $d 'DEFAULT_DEVICE_TREE="as7760-32x"'
      off as7760 $d FSL_SATA_V2 AS7772_BOARD
    done
    on as7760-chainload ${uboots.as7760-chainload} CMD_SPLTEST=y
    # The tree's build. The AS7760 itself still runs 2c0d7387 (flashed before the
    # 2026-11-16 CC_VERSION_STRING change; devices/edge-core-7760-32x/PROGRESS.md).
    pin as7760-nor ${uboots.as7760-nor} 595d81bf49dfe0fd5ed33cd4d5bdd93f20fdb6fae14af2153de094f5884487c3

    # AS7772-32X (AS7760 derivative): same shape, own DT, SATA + GPT for the SSD.
    board_nor as7772-nor ${uboots.as7772-nor}
    board_chainload as7772-chainload ${uboots.as7772-chainload}
    for d in ${uboots.as7772-nor} ${uboots.as7772-chainload}; do
      on as7772 $d AS7772_BOARD=y 'DEFAULT_DEVICE_TREE="as7772-32x"' FSL_SATA_V2=y EFI_PARTITION=y
    done
    # In the AS7772's boot NOR since 2026-11-21 (devices/edge-core-as7772-32x/u-boot-port.md).
    pin as7772-nor ${uboots.as7772-nor} 21718a87211a5e4fbca87b675dd78fe9659dd2e5cbebdb0a105530ba4a3ee1aa
    mkdir $out; echo pass > $out/result
  '';
  qemu = pkgs.runCommand "t208x-uboot-qemu-e6500" {
    nativeBuildInputs = [ pkgs.qemu pkgs.python3 ];
  } ''
    set -euo pipefail
    python3 ${./qemu-test.py} ${pkgs.qemu}/bin/qemu-system-ppc64 ${uboots.qemu-ppce500}/u-boot | tee log.txt
    mkdir $out; cp log.txt $out/
  '';
in pkgs.linkFarm "t208x-uboot-tests" [
  { name = "static"; path = static; }
  { name = "qemu-e6500"; path = qemu; }
]
