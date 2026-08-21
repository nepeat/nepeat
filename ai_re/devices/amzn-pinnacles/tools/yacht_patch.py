#!/usr/bin/env python3
"""
yacht: patch the preloader's secure-chip query so usbdl_verify_da takes the
"DA validation disabled on non-secure chip" path, then let a DA upload proceed.

Usage:  power the tablet OFF (hold power ~10s), start this script, then plug USB
        / power on.  It polls for the preloader and does everything itself.
"""
import logging, sys, time
from mtkclient.config.mtk_config import MtkConfig
from mtkclient.Library.mtk_class import Mtk

CHIPID_REG   = 0x08000000   # oracle: must read back 0x788 (MT8183 hwcode)
CHIPID_EXPECT= 0x788
PL_BASE      = 0x00200D00   # preloader GFH load address
SECCHK_FILE  = 0x2cbb8      # file offset of the secure-chip query
SECCHK_ADDR  = PL_BASE + SECCHK_FILE          # -> 0x0022D8B8
PATCH        = [0x2000, 0x4770]               # movs r0,#0 ; bx lr
SECCHK_BEFORE= 0x68184b02   # "ldr r3,[pc,#8] / ldr r0,[r3]" -- validates PL_BASE
SECCHK_AFTER = 0x47702000   # "movs r0,#0 / bx lr"

def main():
    cfg = MtkConfig(loglevel=logging.INFO, gui=None)
    mtk = Mtk(config=cfg, loglevel=logging.INFO)
    print("[*] waiting for preloader … (power on the device now)")
    if not mtk.preloader.init():
        print("[-] preloader handshake failed"); return 1
    print(f"[+] connected. hwcode=0x{cfg.hwcode:x}")

    v, _ = mtk.preloader.read32(CHIPID_REG, 1)
    val = v[0] if isinstance(v, list) else v
    print(f"[*] ORACLE  READ32 0x{CHIPID_REG:08x} = 0x{val:x} (expect 0x{CHIPID_EXPECT:x})")
    if val != CHIPID_EXPECT:
        print("[-] oracle mismatch — addressing wrong, refusing to patch"); return 1
    print("[+] oracle OK — memory reads are working")

    before, _ = mtk.preloader.read32(SECCHK_ADDR, 1)
    b = before[0] if isinstance(before, list) else before
    print(f"[*] before  0x{SECCHK_ADDR:08x} = 0x{b:08x} (expect 0x{SECCHK_BEFORE:08x})")
    if b != SECCHK_BEFORE:
        print("[-] target does not contain the expected secure-chip query.")
        print("    PL_BASE is wrong or the preloader differs — refusing to patch.")
        return 1
    print("[+] target verified — this is the secure-chip query")

    print(f"[*] patching {[hex(x) for x in PATCH]} -> movs r0,#0 ; bx lr")
    mtk.preloader.write16(SECCHK_ADDR, PATCH)

    after, _ = mtk.preloader.read32(SECCHK_ADDR, 1)
    a = after[0] if isinstance(after, list) else after
    print(f"[*] after   0x{SECCHK_ADDR:08x} = 0x{a:08x}")
    if a == SECCHK_AFTER:
        print("[+] PATCH VERIFIED — secure-chip query now returns 0")
        print("[+] now run:  mtk.py printgpt      (DA should be accepted)")
        return 0
    print("[-] patch did not take"); return 1

if __name__ == "__main__":
    sys.exit(main())
