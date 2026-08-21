#!/usr/bin/env python3
"""
yacht: patch the preloader's daa_enabled query so usbdl_verify_da takes the
"DA validation disabled on non-secure chip" path, then let a DA upload proceed.

Usage:  power the tablet OFF (hold power ~10s), start this script, then plug USB
        / power on.  It polls for the preloader and does everything itself.

The gate is at file offset 0x2cbb8 -> runtime 0x0022D8B8.  It is one of THREE
near-identical fuse stubs 16 bytes apart, all reading 0x11f10060 and all starting
with the same 4 bytes, so the address is pinned by the ubfx at +4, not by +0.

    0x2cba8  ubfx r0,r0,#1,#1   -> sbc_enabled   (security block +8)
    0x2cbb8  ubfx r0,r0,#2,#1   -> daa_enabled   (security block +9)   <-- TARGET
    0x2cbc8  and  r0,r0,#1
"""
import logging, sys
from mtkclient.config.mtk_config import MtkConfig
from mtkclient.Library.mtk_class import Mtk

CHIPID_REG    = 0x08000000   # oracle 1: must read back 0x788 (MT8183 hwcode)
CHIPID_EXPECT = 0x788
PL_BASE       = 0x00200D00   # preloader GFH load address
DAA_FILE      = 0x2cbb8      # file offset of the daa_enabled query
DAA_ADDR      = PL_BASE + DAA_FILE            # -> 0x0022D8B8
PATCH         = [0x2000, 0x4770]              # movs r0,#0 ; bx lr

# oracle 2: the stub prologue. NOT unique -- identical at -0x10 and +0x10.
STUB_HEAD     = 0x68184b02   # ldr r3,[pc,#8] ; ldr r0,[r3]
# oracle 3: the ubfx words. These DO differ, and pin the address to the byte.
UBFX_SBC      = 0x0040f3c0   # ubfx r0,r0,#1,#1   (at DAA_ADDR-0x10)
UBFX_DAA      = 0x0080f3c0   # ubfx r0,r0,#2,#1   (at DAA_ADDR+4)
AND_BIT0       = 0x0001f000  # and r0,r0,#1       (at DAA_ADDR+0x10)

PATCHED_HEAD  = 0x47702000   # movs r0,#0 ; bx lr


def rd32(mtk, addr):
    v, _ = mtk.preloader.read32(addr, 1)
    return v[0] if isinstance(v, list) else v


def main():
    cfg = MtkConfig(loglevel=logging.INFO, gui=None)
    mtk = Mtk(config=cfg, loglevel=logging.INFO)
    print("[*] waiting for preloader … (power on the device now)")
    if not mtk.preloader.init():
        print("[-] preloader handshake failed"); return 1
    print(f"[+] connected. hwcode=0x{cfg.hwcode:x}")

    # ---- oracle 1: do memory reads work at all? ----
    val = rd32(mtk, CHIPID_REG)
    print(f"[*] oracle1  READ32 0x{CHIPID_REG:08x} = 0x{val:x} (expect 0x{CHIPID_EXPECT:x})")
    if val != CHIPID_EXPECT:
        print("[-] oracle1 mismatch — reads not working, refusing to patch"); return 1

    # ---- oracle 2: is PL_BASE right (is there a stub here at all)? ----
    head = rd32(mtk, DAA_ADDR)
    print(f"[*] oracle2  +0x00 = 0x{head:08x} (expect 0x{STUB_HEAD:08x})")
    if head != STUB_HEAD:
        print("[-] no fuse stub at target — PL_BASE wrong. refusing to patch"); return 1

    # ---- oracle 3: WHICH stub is it? this is the one that actually matters ----
    ubfx = rd32(mtk, DAA_ADDR + 4)
    prev = rd32(mtk, DAA_ADDR - 0x10 + 4)
    nxt  = rd32(mtk, DAA_ADDR + 0x10 + 4)
    print(f"[*] oracle3  -0x0c = 0x{prev:08x} (expect 0x{UBFX_SBC:08x}  sbc)")
    print(f"[*] oracle3  +0x04 = 0x{ubfx:08x} (expect 0x{UBFX_DAA:08x}  daa)")
    print(f"[*] oracle3  +0x14 = 0x{nxt:08x} (expect 0x{AND_BIT0:08x}  bit0)")
    if (ubfx, prev, nxt) != (UBFX_DAA, UBFX_SBC, AND_BIT0):
        print("[-] stub identity mismatch — this is NOT the daa_enabled query.")
        print("    (all three stubs share the same first 4 bytes; +0 alone")
        print("     cannot tell them apart.) refusing to patch.")
        return 1
    print("[+] all three oracles pass — target is daa_enabled, address is exact")

    print(f"[*] patching {[hex(x) for x in PATCH]} -> movs r0,#0 ; bx lr")
    mtk.preloader.write16(DAA_ADDR, PATCH)

    a_head = rd32(mtk, DAA_ADDR)
    a_ubfx = rd32(mtk, DAA_ADDR + 4)
    print(f"[*] after   +0x00 = 0x{a_head:08x} (expect 0x{PATCHED_HEAD:08x})")
    print(f"[*] after   +0x04 = 0x{a_ubfx:08x} (expect 0x{UBFX_DAA:08x}, untouched)")
    if a_head != PATCHED_HEAD:
        print("[-] patch did not take"); return 1
    if a_ubfx != UBFX_DAA:
        print("[-] collateral damage at +4 — write was wider than intended"); return 1
    print("[+] PATCH VERIFIED — daa_enabled now returns 0")
    print("[+] now run:  mtk.py printgpt      (DA should be accepted)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
