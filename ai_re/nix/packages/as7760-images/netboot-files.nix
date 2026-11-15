{ pkgs, config, kernelImage }:
# Volatile full-NixOS bundle for TFTP + bootm. U-Boot stages nothing on SD/NOR.
pkgs.runCommand "as7760-netboot-files" {
  nativeBuildInputs = [ pkgs.ubootTools pkgs.dtc pkgs.python3 ];
} ''
  mkdir -p $out
  cp ${kernelImage}/uImage $out/uImage
  export SOURCE_DATE_EPOCH=1
  # Mixed payload (gzip stage-1 cpio + raw squashfs cpio): the kernel unpacks
  # it; U-Boot must not, so the legacy header says "none".
  mkimage -A ppc -O linux -T ramdisk -C none -a 0 -e 0 \
    -n 'AS7760 NixOS netboot' -d ${config.system.build.netbootRamdisk}/initrd $out/uInitrd
  dtc -I dts -O dtb -o $out/as7760.dtb ${builtins.path { path = ../../packages/linux-qoriq-t2080/boards/as7760/as7760-vendor.dts; name = "as7760-vendor.dts"; }} 2> $out/dtc-diagnostics.txt
  echo -n "${pkgs.lib.concatStringsSep " " config.boot.kernelParams} init=${config.system.build.toplevel}/init" > $out/bootargs.txt
  # Volatile U-Boot sequence for booting this bundle from the SD FAT partition.
  # No saveenv: making it the default boot is a separate, explicit decision.
  {
    echo "mmc rescan"
    echo "fatload mmc 0:1 0x10000000 uImage"
    echo "fatload mmc 0:1 0x20000000 uInitrd"
    echo "fatload mmc 0:1 0x08000000 as7760.dtb"
    echo "setenv bootargs $(cat $out/bootargs.txt)"
    echo "bootm 0x10000000 0x20000000 0x08000000"
  } > $out/sd-boot-commands.txt
  # Same sequence as a U-Boot script on the FAT: fatload it to 0x07000000 and
  # "source" it (clear of kernel decompression at 0, initrd, DT staging).
  grep -v '^mmc rescan' $out/sd-boot-commands.txt > boot.cmd
  mkimage -A ppc -O linux -T script -C none -n 'AS7760 SD RAM boot' -d boot.cmd $out/boot.scr
  test "$(wc -c < $out/bootargs.txt)" -lt 230 || { echo "bootargs too long for U-Boot CBSIZE"; exit 1; }
  python3 - $out ${kernelImage}/kernel.bin <<'PY'
  import hashlib, json, struct, sys, zlib
  from pathlib import Path
  out = Path(sys.argv[1]); report = {"hardware_boot_verified": False, "files": {}}
  blob = (out/"uInitrd").read_bytes()
  header = bytearray(blob[:64])
  magic, hcrc, _ts, size, load, entry, dcrc, os_id, arch, typ, comp, _n = struct.unpack(">7I4B32s", header)
  assert magic == 0x27051956 and (os_id, arch, typ, comp) == (5, 7, 3, 0)
  assert len(blob) == size + 64
  header[4:8] = bytes(4)
  assert zlib.crc32(header) == hcrc and zlib.crc32(blob[64:]) == dcrc
  assert blob[64:66] == b"\x1f\x8b", "stage-1 gzip cpio must come first"
  # The kernel only accepts an uncompressed cpio at a 4-byte aligned offset.
  d = zlib.decompressobj(31); d.decompress(blob[64:]); rest = d.unused_data
  cpio_at = len(blob) - 64 - len(rest.lstrip(b"\0"))
  assert cpio_at % 4 == 0 and blob[64 + cpio_at:70 + cpio_at] == b"070701", "squashfs cpio misaligned"
  report["squashfs_cpio_offset"] = cpio_at
  # Staged at 0x20000000; U-Boot relocates it to end at 0x30000000 and the
  # decompressed kernel sits at 0. Keep clear of the 0x10000000 kernel staging.
  assert len(blob) < 0x1c000000, "initrd too large for the 0x20000000 staging plan"
  for name in ("uImage", "uInitrd", "as7760.dtb"):
      data = (out/name).read_bytes()
      report["files"][name] = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
  report["bootargs"] = (out/"bootargs.txt").read_text()
  (out/"manifest.json").write_text(json.dumps(report, indent=2) + "\n")
  print(json.dumps(report, indent=2))
  PY
''
