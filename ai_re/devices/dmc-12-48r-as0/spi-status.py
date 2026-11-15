#!/usr/bin/env python3
"""Read the MX25U12835F ID, status, config and security registers. Linux, read-only.

Run as root on the DMC-12 under NixOS (python3 is in profile-appliance):
    ./spi-status.py            # both chips, spi0.0 (CS0, boots) and spi0.1 (CS1)
    ./spi-status.py 1          # only spi0.1

How: for each chip it unbinds spi-nor, binds spidev via driver_override, sends ONLY
the read opcodes below, then unbinds spidev and reprobes so spi-nor takes the chip back
(/dev/mtdN disappears for well under a second). Untested on hardware (2026-11-15).
Needs CONFIG_SPI_SPIDEV (kept by trim-gen.py). Never add an opcode here: WRSR (01),
WPSEL (68), WRSCUR (2F) and ENSO (B1) change non-volatile or one-time bits.
See spi-recovery.md.
"""
import fcntl, os, struct, sys, time, array

READ_OPS = {  # opcode: (name, bytes to read back)
    0x9F: ("RDID", 3),    # expect c2 25 38 (Macronix, MX25U12835F)
    0x05: ("RDSR", 1),    # bit7 SRWD, bit6 QE, bits5-2 BP3..BP0, bit1 WEL, bit0 WIP
    0x15: ("RDCR", 1),    # bit3 TB (OTP), bits2-0 output driver strength
    0x2B: ("RDSCUR", 1),  # bit7 WPSEL (OTP), bit1 LDSO (OTP), bit0 factory-OTP lock
}
SPI_IOC_MESSAGE_1 = 0x40206B00
SYS = "/sys/bus/spi"


def wr(path, val):
    with open(path, "w") as f:
        f.write(val)


def xfer(fd, op, n):
    assert op in READ_OPS
    tx = array.array("B", [op] + [0] * n)
    rx = array.array("B", [0] * (n + 1))
    t = struct.pack("QQIIHBBBBBB", tx.buffer_info()[0], rx.buffer_info()[0], n + 1,
                    1000000, 0, 8, 0, 0, 0, 0, 0)
    fcntl.ioctl(fd, SPI_IOC_MESSAGE_1, t)
    return bytes(rx[1:])


def decode(sr, cr, scur):
    bp = (sr >> 2) & 0xF
    return (f"SRWD={sr >> 7} QE={(sr >> 6) & 1} BP={bp:04b} WEL={(sr >> 1) & 1} WIP={sr & 1}"
            f" | TB={(cr >> 3) & 1} | WPSEL={scur >> 7} LDSO={(scur >> 1) & 1}"
            f" | {'UNPROTECTED' if bp == 0 and not scur >> 7 else 'PROTECTION SET'}")


def one(cs):
    dev = f"spi0.{cs}"
    d = f"{SYS}/devices/{dev}"
    if not os.path.exists(d):
        print(f"{dev}: not present"); return
    os.system("modprobe spidev 2>/dev/null")
    drv = os.path.basename(os.path.realpath(f"{d}/driver")) if os.path.exists(f"{d}/driver") else None
    try:
        if drv:
            wr(f"{SYS}/drivers/{drv}/unbind", dev)
        wr(f"{d}/driver_override", "spidev")
        wr(f"{SYS}/drivers/spidev/bind", dev)
        time.sleep(0.2)
        node = [x for x in os.listdir(f"{d}/spidev")][0]
        fd = os.open(f"/dev/{node}", os.O_RDWR)
        try:
            r = {op: xfer(fd, op, n) for op, (_, n) in READ_OPS.items()}
        finally:
            os.close(fd)
        print(f"{dev}: RDID {r[0x9F].hex()} SR {r[0x05][0]:02x} CR {r[0x15][0]:02x}"
              f" SCUR {r[0x2B][0]:02x}")
        print(f"{dev}: {decode(r[0x05][0], r[0x15][0], r[0x2B][0])}")
    finally:
        if os.path.exists(f"{d}/driver"):
            wr(f"{SYS}/drivers/spidev/unbind", dev)
        wr(f"{d}/driver_override", "\n")
        if drv:
            wr(f"{SYS}/drivers_probe", dev)
        print(f"{dev}: driver now "
              f"{os.path.basename(os.path.realpath(d + '/driver')) if os.path.exists(d + '/driver') else 'NONE'}")


if __name__ == "__main__":
    for cs in (sys.argv[1:] or ["0", "1"]):
        one(int(cs))
