{ pkgs, uboots }:
# "Does this U-Boot run on the T2080's PowerPC core?" checks that need no board:
#  static: ELF32 big-endian PowerPC, expected entry, built for -mcpu=e6500,
#          no SPE (e500v2-only) opcodes, safety-relevant config for chainload,
#          NOR image size/reset vector identical in shape to the factory one.
#  qemu:   boot the same tree (qemu-ppce500) on an emulated e6500 core and
#          drive the shell (version, bdinfo, memory read/write/compare).
let
  binutils = pkgs.pkgsCross.ppc32.buildPackages.binutils-unwrapped;
  static = pkgs.runCommand "as7760-uboot-static-checks" {
    nativeBuildInputs = [ binutils pkgs.python3 pkgs.gnugrep ];
  } ''
    set -euo pipefail
    check() { # name dir entry
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
    # NOR: ELF entry is the boot page (the reset vector at 0xeffffffc jumps there).
    check t2080rdb-nor ${uboots.t2080rdb-nor} 0xeffff000
    check as7760-chainload ${uboots.as7760-chainload} 0x200000
    check as7760-nor ${uboots.as7760-nor} 0xeffff000
    # NOR image: exactly the factory partition size, ends in the reset-vector branch.
    python3 - ${uboots.t2080rdb-nor}/u-boot.bin <<'PY'
    import sys
    b = open(sys.argv[1], "rb").read()
    assert len(b) == 786432, len(b)
    assert b[-4:] == bytes.fromhex("4bfff004"), b[-4:].hex()
    print("t2080rdb-nor: 768 KiB, reset vector b .-0xffc")
    PY
    # Chainload build must not touch hardware we have not identified.
    c=${uboots.as7760-chainload}/.config
    for k in VID MISC_INIT_R USB FSL_SATA_V2; do
      ! grep -q "^CONFIG_$k=y" $c || { echo "chainload: CONFIG_$k is on"; exit 1; }
    done
    grep -qx CONFIG_CHAINLOAD_CCSR_RELOCATED=y $c; grep -qx 'CONFIG_BOOTDELAY=2' $c; grep -q 'bootflow scan -lb' $c
    grep -qx 'CONFIG_SYS_CLK_FREQ=100000000' $c
    ! powerpc-unknown-linux-gnu-nm ${uboots.as7760-chainload}/u-boot | grep -qw adjust_vdd
    echo "chainload: safety config ok"
    # NOR replacement image: same layout as the factory U-Boot partition.
    n=${uboots.as7760-nor}/.config
    for k in VID MISC_INIT_R CHAINLOAD_CCSR_RELOCATED CMD_SPLTEST; do
      ! grep -q "^CONFIG_$k=y" $n || { echo "nor: CONFIG_$k is on"; exit 1; }
    done
    for kv in TEXT_BASE=0xEFF40000 SYS_MONITOR_LEN=786432 ENV_IS_IN_FLASH=y ENV_ADDR=0xEFF20000 \
              ENV_SIZE=0x2000 ENV_SECT_SIZE=0x20000 SYS_QE_FMAN_FW_IN_NOR=y SYS_FMAN_FW_ADDR=0xEFF00000 \
              SYS_CLK_FREQ=100000000 AS7760_DDR=y DIMM_SLOTS_PER_CTLR=2 PCI=y; do
      grep -qx "CONFIG_$kv" $n || { echo "nor: CONFIG_$kv missing"; exit 1; }
    done
    python3 - ${uboots.as7760-nor}/u-boot.bin <<'PY'
    import sys
    b = open(sys.argv[1], "rb").read()
    assert len(b) == 786432, len(b)
    assert b[-4:] == bytes.fromhex("4bfff004"), b[-4:].hex()
    print("as7760-nor: 768 KiB, reset vector b .-0xffc")
    PY
    ! powerpc-unknown-linux-gnu-nm ${uboots.as7760-nor}/u-boot | grep -qw adjust_vdd
    echo "nor: config ok"
    # AS7772-32X (AS7760 derivative): same shape, own DT, SATA + GPT for the SSD.
    check as7772-chainload ${uboots.as7772-chainload} 0x200000
    check as7772-nor ${uboots.as7772-nor} 0xeffff000
    for d in ${uboots.as7772-nor} ${uboots.as7772-chainload}; do
      for k in VID MISC_INIT_R USB; do
        ! grep -q "^CONFIG_$k=y" $d/.config || { echo "as7772: CONFIG_$k is on"; exit 1; }
      done
      for kv in AS7760_BOARD=y AS7772_BOARD=y AS7760_DDR=y 'DEFAULT_DEVICE_TREE="as7772-32x"' FSL_SATA_V2=y \
                EFI_PARTITION=y SYS_CLK_FREQ=100000000 DIMM_SLOTS_PER_CTLR=2; do
        grep -qx "CONFIG_$kv" $d/.config || { echo "as7772: CONFIG_$kv missing"; exit 1; }
      done
      ! powerpc-unknown-linux-gnu-nm $d/u-boot | grep -qw adjust_vdd
    done
    n=${uboots.as7772-nor}/.config
    for kv in TEXT_BASE=0xEFF40000 SYS_MONITOR_LEN=786432 ENV_IS_IN_FLASH=y ENV_ADDR=0xEFF20000 \
              ENV_SIZE=0x2000 ENV_SECT_SIZE=0x20000 SYS_QE_FMAN_FW_IN_NOR=y SYS_FMAN_FW_ADDR=0xEFF00000; do
      grep -qx "CONFIG_$kv" $n || { echo "as7772-nor: CONFIG_$kv missing"; exit 1; }
    done
    ! grep -q "^CONFIG_CHAINLOAD_CCSR_RELOCATED=y" $n
    python3 - ${uboots.as7772-nor}/u-boot.bin <<'PY'
    import sys
    b = open(sys.argv[1], "rb").read()
    assert len(b) == 786432, len(b)
    assert b[-4:] == bytes.fromhex("4bfff004"), b[-4:].hex()
    print("as7772-nor: 768 KiB, reset vector b .-0xffc")
    PY
    echo "as7772: config ok"
    mkdir $out; echo pass > $out/result
  '';
  qemu = pkgs.runCommand "as7760-uboot-qemu-e6500" {
    nativeBuildInputs = [ pkgs.qemu pkgs.python3 ];
  } ''
    set -euo pipefail
    python3 ${./qemu-test.py} ${pkgs.qemu}/bin/qemu-system-ppc64 ${uboots.qemu-ppce500}/u-boot | tee log.txt
    mkdir $out; cp log.txt $out/
  '';
in pkgs.linkFarm "as7760-uboot-tests" [
  { name = "static"; path = static; }
  { name = "qemu-e6500"; path = qemu; }
]
