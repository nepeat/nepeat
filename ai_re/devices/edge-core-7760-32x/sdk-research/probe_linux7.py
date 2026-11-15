#!/usr/bin/env python3
"""Compile-only x86_64 Linux 7 API probe. Never load these into the switch.
Run inside nix develop. Apply linux7-module-api.patch to the pinned source first.
"""
import argparse
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("kernel", type=Path)
parser.add_argument("--cc", required=True)
parser.add_argument("--pahole", required=True)
args = parser.parse_args()
sdk = root / "sources/saibcm-modules"
build = root / "build/kbuild-7.3.4"
build.mkdir(parents=True, exist_ok=True)
if subprocess.check_output([args.cc, "-dumpmachine"], text=True).strip() != "x86_64-unknown-linux-gnu":
    raise SystemExit("This probe requires a native x86_64 compiler")
if '"x86_64"' not in (args.kernel / "include/generated/compile.h").read_text():
    raise SystemExit("This probe requires an x86_64 configured kernel")
groups = {
    "linux-kernel-bde": {
        "kernel-main": "systems/bde/linux/kernel/linux-kernel-bde.c",
        "linux_shbde": "systems/bde/linux/kernel/linux_shbde.c",
        "linux_dma": "systems/bde/linux/kernel/linux_dma.c",
        "mpool": "systems/bde/linux/shared/mpool.c",
        "shbde_pci": "systems/bde/shared/shbde_pci.c",
        "shbde_iproc": "systems/bde/shared/shbde_iproc.c",
        "shbde_mdio": "systems/bde/shared/shbde_mdio.c",
        "kernel-gmodule": "systems/linux/kernel/modules/shared/gmodule.c",
    },
    "linux-user-bde": {
        "user-main": "systems/bde/linux/user/kernel/linux-user-bde.c",
        "user-gmodule": "systems/linux/kernel/modules/shared/gmodule.c",
    },
    "linux-bcm-knet": {
        "knet-main": "systems/linux/kernel/modules/bcm-knet/bcm-knet.c",
        "knet-gmodule": "systems/linux/kernel/modules/shared/gmodule.c",
    },
}
includes = ["include", "systems/bde/linux/include", "systems/bde/shared/include",
            "systems/linux/kernel/modules/include", "systems/bde/linux/kernel",
            "systems/bde/linux/shared", "systems/bde/linux/user/kernel",
            "systems/linux/kernel/modules/bcm-knet"]
lines = ["ccflags-y := " + " ".join("-I" + str(sdk / x) for x in includes),
         "ccflags-y += -DLINUX -DNO_FILEIO -DNO_CTRL_C -DNO_MEMTUNE -DPTRS_ARE_64BITS -DLONGS_ARE_64BITS -DPHYS_ADDRS_ARE_64BITS -DSYS_BE_PIO=0 -DSYS_BE_PACKET=0 -DSYS_BE_OTHER=0 -DLE_HOST=1 -DUSE_LINUX_BDE_MMAP=1 -DSAL_BDE_DMA_MEM_DEFAULT=192"]
for module, files in groups.items():
    lines += [f"obj-m += {module}.o", f"{module}-y := " + " ".join(x + ".o" for x in files)]
    for name, source in files.items():
        dest = build / (name + ".c")
        target = sdk / source
        if dest.is_symlink():
            if dest.resolve() != target.resolve():
                raise SystemExit(f"Unexpected symlink: {dest}")
        elif dest.exists():
            raise SystemExit(f"Refusing to replace {dest}")
        else:
            dest.symlink_to(target)
lines += ["ccflags-y += -include " + str(root / "linux7-compat.h")]
(build / "Makefile").write_text("\n".join(lines) + "\n")
subprocess.run(["make", "-C", str(args.kernel.resolve()), "M=" + str(build),
                "CC=" + args.cc, "PAHOLE=" + args.pahole, "-j4", "modules"], check=True)
