#!/usr/bin/env python3
"""Hold or release the 8 Credo CMS50216 MACsec PHY chips through the MDIO FPGA.

Same access as the vendor mdio-fpga.ko phyN_reset attribute: 32-bit
read-modify-write of BAR0 + 0x80000 + 0x20*(N-1), bit 0 low = chip in reset.
Firmware and configuration are lost in reset; cs8210-switch-up reloads them.
"""
import mmap
import os
import sys

DEV = "/sys/bus/pci/devices/0004:01:00.0"


def main():
    want = sys.argv[1] if len(sys.argv) > 1 else "status"
    if want not in ("status", "reset", "release"):
        sys.exit("usage: cs8210-credo-reset [status|reset|release]")
    cfg = open(DEV + "/config", "rb").read(6)
    if not int.from_bytes(cfg[4:6], "little") & 2:
        # Memory decode is off after U-Boot; a BAR access then is an SError.
        with open(DEV + "/enable", "w") as f:
            f.write("1")
    fd = os.open(DEV + "/resource0", os.O_RDWR | os.O_SYNC)
    m = mmap.mmap(fd, 0x100000, mmap.MAP_SHARED, mmap.PROT_READ | mmap.PROT_WRITE)
    v = memoryview(m).cast("I")
    if v[0] != 0x00001008:
        sys.exit("unexpected MDIO FPGA version 0x%08x" % v[0])
    for n in range(1, 9):
        i = (0x80000 + 0x20 * (n - 1)) // 4
        cur = v[i]
        if want == "reset":
            v[i] = cur & ~1
        elif want == "release":
            v[i] = cur | 1
        print("credo chip %d: %s" % (n, "running" if v[i] & 1 else "in reset"))


if __name__ == "__main__":
    main()
