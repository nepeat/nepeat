#!/usr/bin/env python3
# devmem.py words <addr> <count>      -- print 32-bit LE words (read via mmap of /dev/mem)
# devmem.py dump  <addr> <len> <file> -- copy bytes to file, 32-bit accesses
import mmap, os, struct, sys
PAGE = 4096
def rd(addr, n):
    base = addr & ~(PAGE - 1)
    span = (addr + n - base + PAGE - 1) & ~(PAGE - 1)
    fd = os.open("/dev/mem", os.O_RDONLY | os.O_SYNC)
    m = mmap.mmap(fd, span, mmap.MAP_SHARED, mmap.PROT_READ, offset=base)
    out = bytearray()
    off = addr - base
    for i in range(0, n, 4):
        out += m[off + i: off + i + 4]   # 4-byte slice -> one word-sized copy
    m.close(); os.close(fd)
    return bytes(out)
mode, addr = sys.argv[1], int(sys.argv[2], 0)
if mode == "words":
    cnt = int(sys.argv[3], 0)
    d = rd(addr, cnt * 4)
    for i in range(0, cnt, 4):
        ws = struct.unpack_from("<%dI" % min(4, cnt - i), d, i * 4)
        print("%08x: %s" % (addr + i * 4, " ".join("%08x" % w for w in ws)))
else:
    ln = int(sys.argv[3], 0)
    open(sys.argv[4], "wb").write(rd(addr, ln))
