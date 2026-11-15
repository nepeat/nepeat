{ pkgs, targetPkgs, kernel }:
pkgs.runCommand "as7760-uImage-${kernel.version}" {
  nativeBuildInputs = [ pkgs.ubootTools pkgs.gzip pkgs.python3 ];
} ''
  mkdir -p $out
  ${targetPkgs.stdenv.cc.bintools}/bin/${targetPkgs.stdenv.cc.targetPrefix}objcopy \
    -O binary ${kernel}/vmlinux $out/kernel.bin
  gzip -n -9 < $out/kernel.bin > kernel.bin.gz
  export SOURCE_DATE_EPOCH=1
  mkimage -A ppc -O linux -T kernel -C gzip -a 0 -e 0 \
    -n 'AS7760 Linux ${kernel.version}' -d kernel.bin.gz $out/uImage
  python3 - ${kernel}/vmlinux $out <<'PY'
  import gzip, hashlib, json, struct, sys, zlib
  from pathlib import Path
  elf = Path(sys.argv[1]).read_bytes()
  assert elf[:6] == b'\x7fELF\x02\x02', 'expected ELF64 big endian'
  assert struct.unpack_from('>H', elf, 18)[0] == 21, 'expected PowerPC64'
  assert struct.unpack_from('>Q', elf, 24)[0] == 0xc000000000000000
  out = Path(sys.argv[2]); blob = (out/'uImage').read_bytes()
  header = bytearray(blob[:64]); fields = struct.unpack('>7I4B32s', header)
  magic, hcrc, timestamp, size, load, entry, dcrc, os_id, arch, typ, comp, label = fields
  assert magic == 0x27051956 and (os_id, arch, typ, comp) == (5, 7, 2, 1)
  assert load == entry == 0 and len(blob) == size + 64
  header[4:8] = bytes(4)
  assert zlib.crc32(header) == hcrc and zlib.crc32(blob[64:]) == dcrc
  raw = (out/'kernel.bin').read_bytes()
  assert gzip.decompress(blob[64:]) == raw
  assert len(raw) < 64 * 1024**2, 'exceeds reference U-Boot gunzip limit'
  report = {'linux': '${kernel.version}', 'architecture': 'powerpc64 big-endian e6500',
            'raw_bytes': len(raw), 'uImage_bytes': len(blob),
            'load': load, 'entry': entry, 'hardware_boot_verified': False,
            'sha256': hashlib.sha256(blob).hexdigest()}
  (out/'manifest.json').write_text(json.dumps(report, indent=2)+'\n')
  print(json.dumps(report, indent=2))
  PY
''
