#!/usr/bin/env python3
"""Validate and collect the compiled RAM-only OpenWrt netboot artifacts."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import zlib

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT / "work/source"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", default="10.60.2.208")
    parser.add_argument("--address", default="10.60.2.203", help="switch's existing lab address")
    args = parser.parse_args()
    images = list((SOURCE / "bin/targets/mpc85xx/bcm98548").glob("*-initramfs.uImage"))
    if len(images) != 1:
        raise SystemExit(f"Expected one completed initramfs uImage, found {images}")
    data = images[0].read_bytes()
    header = bytearray(data[:64])
    magic, expected_header, timestamp, size, load, entry, expected_data = struct.unpack_from(">7I", header)
    header[4:8] = b"\0" * 4
    if (magic != 0x27051956 or zlib.crc32(header) != expected_header
            or len(data) != size + 64 or zlib.crc32(data[64:]) != expected_data
            or data[28:32] != bytes((5, 7, 2, 1)) or load or entry):
        raise SystemExit("Unexpected/corrupt uImage; refusing to package")
    unpacked = gzip.decompress(data[64:])  # Verify gzip integrity as well as uImage CRCs.
    if len(unpacked) >= 0x02000000:
        raise SystemExit("Kernel exceeds 32 MiB; re-evaluate the 48 MiB DTB placement")
    configs = list((SOURCE / "build_dir").glob("target-*/linux-mpc85xx_bcm98548/linux-*/.config"))
    if len(configs) != 1:
        raise SystemExit("Missing/unexpected kernel configuration")
    # Include zero-filled BSS in the layout check: objcopy's raw image
    # length alone does not describe all memory occupied by the kernel.
    elf = (configs[0].parent / "vmlinux").read_bytes()
    if elf[:6] != b"\x7fELF\x01\x02":
        raise SystemExit("Expected an ELF32 big-endian kernel")
    phoff = struct.unpack_from(">I", elf, 28)[0]
    phsize, phcount = struct.unpack_from(">HH", elf, 42)
    for index in range(phcount):
        segment = struct.unpack_from(">8I", elf, phoff + index * phsize)
        if segment[0] == 1 and segment[3] + segment[5] > 0x03000000:
            raise SystemExit("Kernel load segment overlaps the 48 MiB DTB")
    config = configs[0].read_text()
    for symbol in ("MTD", "DEVMEM"):
        if f"CONFIG_{symbol}=y" in config or f"CONFIG_{symbol}=m" in config:
            raise SystemExit(f"CONFIG_{symbol} enabled; RAM-only prototype policy failed")
    target_roots = list((SOURCE / "build_dir").glob("target-*/root-mpc85xx"))
    if len(target_roots) != 1:
        raise SystemExit("Missing built root filesystem")
    if "CONFIG_PCI=y" in config:
        if not list(target_roots[0].glob("lib/modules/*/bcm-bde-compat.ko")):
            raise SystemExit("ASIC image is missing the BDE compatibility module")
        if not (target_roots[0] / "usr/lib/bcm-sdk/provenance.json").is_file():
            raise SystemExit("ASIC image is missing the recovered SDK provenance")
    for name in ("usr/lib/os-release", "etc/openwrt_release", "etc/banner"):
        if "(emma’s version)" not in (target_roots[0] / name).read_text():
            raise SystemExit(f"Requested branding is missing from {name}")
    metadata = json.loads((target_roots[0] / "etc/emma-build.json").read_text())
    if metadata.get("label") != "(emma’s version)":
        raise SystemExit("Requested branding is missing from build metadata")
    destination = ROOT / "artifacts"
    destination.mkdir(exist_ok=True)
    image = destination / "emma-openwrt-netboot.uImage"
    shutil.copyfile(images[0], image)
    dtc = shutil.which("dtc") or str(configs[0].parent / "scripts/dtc/dtc")
    tree = destination / "bcm98548xmc-netboot.dtb"
    subprocess.run([dtc, "-I", "dts", "-O", "dtb", "-o", str(tree),
                    str(ROOT / "overlay/target/linux/mpc85xx/files/arch/powerpc/boot/dts/bcm98548xmc-netboot.dts")], check=True)
    commands = (
        "# At CFE>, temporary settings only. No flash/save/persistent commands.\n"
        f"ifconfig eth0 -addr={args.address} -mask=255.255.255.0 -gw=10.60.2.1\n"
        f"load -raw -addr=0x03000000 -max=0x10000 -tftp {args.server}:emma-openwrt/bcm98548xmc-netboot.dtb\n"
        'setenv LINUX_CMDLINE "console=ttyS0,9600 earlycon=uart8250,mmio,0xe0004500"\n'
        f"boot -uboot -extdtb -dtbaddr=0x03000000 -tftp {args.server}:emma-openwrt/emma-openwrt-netboot.uImage\n"
    )
    (destination / "cfe-netboot.txt").write_text(commands)
    manifest = {"label": "(emma’s version)", "status": "compiled; hardware boot not implied",
                "openwrt_version": "25.12.1", "board": "BCM98548XMC",
                "boot": "TFTP/RAM initramfs", "flash_drivers": False,
                "dtb_address": "0x03000000", "unpacked_image_bytes": len(unpacked),
                "asic_support": "experimental legacy SDK/BDE integration" if "CONFIG_PCI=y" in config else False,
                "default_lane_profile": "16x40G", "luci": True,
                "packet_interface": json.loads((target_roots[0] / "usr/lib/bcm-sdk/provenance.json").read_text()).get("packet_interface", "none"),
                "files": {}}
    for path in (image, tree):
        manifest["files"][path.name] = {"bytes": path.stat().st_size,
                                       "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    (destination / "kernel.config").write_text(config)
    shutil.copyfile(SOURCE / ".config", destination / "openwrt.config")
    print(json.dumps(manifest, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
