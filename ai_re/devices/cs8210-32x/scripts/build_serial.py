#!/usr/bin/env python3
"""Build the untested-on-hardware serial candidate. Run inside ai_re devshell.
Needs analysis/kernel-review/linux-7.2.11 and unpack_onie.py output.
Does not download, install, flash, or boot anything.
"""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import struct


def run(argv, log, **kwargs):
    with log.open("w") as stream:
        subprocess.run([str(a) for a in argv], stdout=stream, stderr=subprocess.STDOUT,
                       check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jobs", type=int, default=8)
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("jobs must be positive")
    project = Path(__file__).resolve().parents[1]
    source = project / "analysis/kernel-review/linux-7.2.11"
    root = project / "analysis/research/onie/rootfs"
    out = project / "analysis/research/serial-build"
    out.mkdir(parents=True, exist_ok=True)
    inputs = [project / "bringup/init", project / "bringup/serial.config",
              project / "bringup/cs8210-serial.dts", root / "bin/busybox"]
    entries = [f"dir /{d} 0755 0 0" for d in ("bin", "lib", "dev", "proc", "sys", "tmp", "root")]
    entries += ["slink /lib64 lib 0777 0 0", "nod /dev/console 0600 0 0 c 5 1", "nod /dev/null 0666 0 0 c 1 3",
                f"file /init {project}/bringup/init 0755 0 0",
                f"file /bin/busybox {root}/bin/busybox 0755 0 0"]
    for soname, real in [("ld-linux-aarch64.so.1", "ld-2.25.so"),
                         ("libc.so.6", "libc-2.25.so"),
                         ("libcrypt.so.1", "libcrypt-2.25.so"),
                         ("libm.so.6", "libm-2.25.so")]:
        inputs.append(root / "lib" / real)
        entries += [f"file /lib/{real} {root}/lib/{real} 0755 0 0",
                    f"slink /lib/{soname} {real} 0777 0 0"]
    for app in ("sh", "mount", "cat", "uname", "ls", "dmesg", "sleep", "hexdump", "grep",
                "head", "tail", "wc", "readlink", "find", "ps", "date", "mkdir", "stty"):
        entries.append(f"slink /bin/{app} busybox 0777 0 0")
    for item in inputs:
        if not item.is_file():
            raise FileNotFoundError(item)
    (out / "initramfs.list").write_text("\n".join(entries) + "\n")
    (out / "all.config").write_text((project / "bringup/serial.config").read_text() +
                                    f'CONFIG_INITRAMFS_SOURCE="{out}/initramfs.list"\n')
    preprocessed = subprocess.check_output([
        "cc", "-E", "-P", "-x", "assembler-with-cpp", "-nostdinc", "-undef", "-D__DTS__",
        "-I", str(source / "arch/arm64/boot/dts"), "-I", str(source / "scripts/dtc/include-prefixes"),
        str(project / "bringup/cs8210-serial.dts")])
    (out / "cs8210-serial.preprocessed.dts").write_bytes(preprocessed)
    run(["dtc", "-I", "dts", "-O", "dtb", "-o", out / "cs8210-serial.dtb",
         out / "cs8210-serial.preprocessed.dts"], out / "dtc.txt")
    env = dict(os.environ, KBUILD_BUILD_USER="cs8210-re", KBUILD_BUILD_HOST="offline",
               KBUILD_BUILD_TIMESTAMP="2026-10-17 00:00:00 UTC", KBUILD_BUILD_VERSION="1")
    make = ["make", "-C", source, f"O={out}", "ARCH=arm64",
            "CROSS_COMPILE=aarch64-unknown-linux-gnu-"]
    run(make + [f"KCONFIG_ALLCONFIG={out}/all.config", "allnoconfig"], out / "configure.txt", env=env)
    config = (out / ".config").read_text()
    for name in ("ARCH_ALPINE", "SMP", "ARM_PSCI_FW", "ARM_GIC_V3", "ARM_ARCH_TIMER",
                 "SERIAL_8250_CONSOLE", "SERIAL_OF_PLATFORM", "SERIAL_AMBA_PL011_CONSOLE",
                 "BINFMT_ELF", "BINFMT_SCRIPT", "BLK_DEV_INITRD", "DEVTMPFS", "PROC_FS", "SYSFS"):
        assert f"CONFIG_{name}=y\n" in config, name
    for name in ("PCI", "NET", "BLOCK", "WATCHDOG", "MTD", "I2C", "SPI"):
        assert f"CONFIG_{name}=y\n" not in config and f"CONFIG_{name}=m\n" not in config, name
    run(make + [f"-j{args.jobs}", "Image"], out / "build.txt", env=env)
    kernel = out / "arch/arm64/boot/Image"
    image = kernel.read_bytes()
    assert image[56:60] == b"ARM\x64"
    text_offset, image_size = struct.unpack_from("<QQ", image, 8)
    # arm64 boot protocol requires text_offset above a 2 MiB aligned base.
    # Modern Image has offset 0, unlike the stock 4.9 Image (0x80000).
    load_address = 0x04000000 + text_offset
    assert load_address + image_size < 0x10000000
    (out / "arm64-header.json").write_text(json.dumps({"text_offset": hex(text_offset),
        "image_size": image_size, "proposed_load_address": hex(load_address),
        "hardware_memory_layout_verified": False}, indent=2) + "\n")
    (out / "Image.gz").write_bytes(gzip.compress(image, mtime=0))
    run(["mkimage", "-A", "arm64", "-O", "linux", "-T", "kernel", "-C", "gzip",
         "-a", f"{load_address:x}", "-e", f"{load_address:x}", "-n", "7.2.11 CS8210 serial EXPERIMENTAL",
         "-d", out / "Image.gz", out / "uImage"], out / "mkimage.txt",
        env=dict(env, SOURCE_DATE_EPOCH="1792195200"))
    outputs = [kernel, out / "Image.gz", out / "uImage", out / "cs8210-serial.dtb", out / ".config"]
    hashes = {str(p.relative_to(project)): {"bytes": p.stat().st_size,
              "sha256": hashlib.sha256(p.read_bytes()).hexdigest()} for p in inputs + outputs}
    (out / "manifest.json").write_text(json.dumps(hashes, indent=2) + "\n")
    print(json.dumps({str(p.relative_to(out)): hashes[str(p.relative_to(project))] for p in outputs}, indent=2))


if __name__ == "__main__":
    main()
