#!/usr/bin/env python3
"""Prepare a pinned OpenWrt board port and RAM-only build configuration."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request
from prepare_auth import prepare_auth
from prepare_sdk import prepare_sdk

ROOT = Path(__file__).resolve().parent
WORK = ROOT / "work"
SOURCE = WORK / "source"
COMMIT = "d6115e42127481645ae7210c9d8ca1bd14313473"
VERSION = "25.12.1"
LABEL = "(emma’s version)"
ARCHIVE = "openwrt-toolchain-25.12.1-mpc85xx-p2020_gcc-14.3.0_musl.Linux-x86_64.tar.zst"
TOOLCHAIN_SHA256 = "b8c806c7f18c976625886db482eebe279445f55d91c92c355ba9571f7c58423a"
TOOLCHAIN = WORK / ARCHIVE.removesuffix(".tar.zst") / "toolchain-powerpc_8548_gcc-14.3.0_musl"


def run(*args):
    subprocess.run(args, check=True)


def main():
    WORK.mkdir(exist_ok=True)
    if not SOURCE.exists():
        run("git", "clone", "--quiet", "--depth", "1", "--branch", "v" + VERSION,
            "https://github.com/openwrt/openwrt.git", str(SOURCE))
    head = subprocess.check_output(["git", "-C", str(SOURCE), "rev-parse", "HEAD"], text=True).strip()
    if head != COMMIT:
        raise SystemExit(f"Expected source {COMMIT}; got {head}. Refusing to change checkout.")
    archive = WORK / ARCHIVE
    if not archive.exists():
        url = f"https://downloads.openwrt.org/releases/{VERSION}/targets/mpc85xx/p2020/{ARCHIVE}"
        temporary = archive.with_suffix(".download")
        with urllib.request.urlopen(url, timeout=120) as src, temporary.open("wb") as dst:
            shutil.copyfileobj(src, dst)
        temporary.rename(archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != TOOLCHAIN_SHA256:
        raise SystemExit("Cross-toolchain checksum mismatch")
    if not TOOLCHAIN.exists():
        run("tar", "--zstd", "-xf", str(archive), "-C", str(WORK))
    for src in (ROOT / "overlay").rglob("*"):
        if src.is_file():
            dst = SOURCE / src.relative_to(ROOT / "overlay")
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
    target = SOURCE / "target/linux/mpc85xx/Makefile"
    text = target.read_text()
    text = text.replace("SUBTARGETS:=p1010 p1020 p2020\n", "SUBTARGETS:=p1010 p1020 p2020 bcm98548\n")
    target.write_text(text)
    os_release = SOURCE / "package/base-files/files/usr/lib/os-release"
    os_release.write_text(os_release.read_text().replace('PRETTY_NAME="%D %V"', 'PRETTY_NAME="%D %V %C"'))
    config = (ROOT / "seed.config").read_text()
    config += '\nCONFIG_DEVEL=y\nCONFIG_EXTERNAL_TOOLCHAIN=y\n'
    config += f'CONFIG_TOOLCHAIN_ROOT="{TOOLCHAIN}"\n'
    config += 'CONFIG_TOOLCHAIN_PREFIX="powerpc-openwrt-linux-musl-"\n'
    config += 'CONFIG_TARGET_NAME="powerpc-openwrt-linux-musl"\n'
    config += 'CONFIG_EXTERNAL_GCC_VERSION="14.3.0"\n'
    config += 'CONFIG_EXTERNAL_TOOLCHAIN_LIBC_USE_MUSL=y\n'
    (SOURCE / ".config").write_text(config)
    shutil.copytree(ROOT / "files", SOURCE / "files", dirs_exist_ok=True)
    prepare_auth(SOURCE, WORK)
    prepare_sdk(SOURCE, ROOT.parent)
    metadata = {"distribution": "OpenWrt", "version": VERSION, "label": LABEL,
                "source_commit": COMMIT, "board": "BCM98548XMC / BCM956845K-02",
                "boot_mode": "TFTP -> RAM initramfs; no persistent install image",
                "toolchain_sha256": TOOLCHAIN_SHA256,
                "asic_support": "experimental BCM56845 BDE compatibility; see hardware test records"}
    destination = SOURCE / "files/etc/emma-build.json"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(metadata, indent=2, ensure_ascii=False) + "\n")
    for compiler in (ROOT / "host-tools").iterdir():
        compiler.chmod(0o755)
    print(json.dumps(metadata, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
