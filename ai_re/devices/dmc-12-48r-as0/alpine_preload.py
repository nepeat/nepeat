#!/usr/bin/env python3
"""Decode an Annapurna Alpine V2 I2C preloader EEPROM image (offline only).

  alpine_preload.py <eeprom.bin | EEPROM/HW_PLD .fimg> [--show-ids]
  alpine_preload.py --strap 0x03bfdef5

The EEPROM at I2C 0x57 (strap bit 23; 0x50 if clear) on a K2 card holds, by offset:
  0x000  hardware preload table: 12-byte entries {u32 data, u64 addr}, header and
         footer entry = 0x1c361c36 / 0x1c361c361c361c36 (the PBS I2C preloader writes
         them before the CPU leaves reset), then a 0x200-byte trailer
  0x400  "early init info" for stage2 (offset taken from the preloader's SRAM record
         at 0xfbff410a/b): SPD locations, DDR impedance (read by stage2 2.44 only)
  0x440  DDR4 SPD of the soldered-down DRAM (on DMC-12; any offset the early info names)
  0x600  al_eeprom_board_data (magic 0xCC: MACs, PCB rev, serial, part number)
Formats and evidence: devices/dmc-12-48r-as0/boot-chain-details.md and
ai_re/alpine-v2-boot-chain.md. MAC/serial/part number print only with --show-ids.
"""
import struct, sys

PLD_MAGIC = 0x1C361C36
PLD_ADDR_MAGIC = 0x1C361C361C361C36
OBJ_MAGIC = 0x000B9EC7
REF_HZ = 100_000_000  # pll_ref_clk_freq on u01 (dev_info); strap bit 19

PLLS = {0xFD860B00: "SB", 0xFD860C00: "NB", 0xFD860D00: "CPU"}  # AL_PLL_BASE(id), ring+0xb00
REGS = {
    0xFD8A8114: "PBS cfg_axi_conf_0", 0xFD8A8118: "PBS cfg_axi_conf_1",
    0xFD8A811C: "PBS cfg_axi_conf_2", 0xFD8A8120: "PBS cfg_axi_conf_3 (timeout)",
    0xFD8A81B4: "PBS cfg_nand_0 (ROM NAND geometry)", 0xFD8A81B8: "PBS cfg_nand_1 (ROM NAND ECC)",
    0xF00700A4: "NB qos_set_control", 0xF00700A0: "NB qos_peak_control",
    0xFD882014: "SPI master BAUDR (SCK divider)",
}


def u32(b, o): return struct.unpack_from("<I", b, o)[0]


def regname(a):
    if a in REGS: return REGS[a]
    for base, n in PLLS.items():
        if base <= a < base + 0x100:
            off = a - base
            if off == 0: return f"{n} PLL setup_0"
            if off == 4: return f"{n} PLL setup_1"
            if 0x20 <= off < 0x40: return f"{n} PLL ch_pair[{(off - 0x20) // 4}].cfg"
            return f"{n} PLL +{off:#x}"
    if 0xFD8B4000 <= a < 0xFD8B5000: return f"SGPO +{a - 0xFD8B4000:#x}"
    if 0xFD8A8138 <= a < 0xFD8A8160: return "PBS MUIO pin mux"
    if 0xFBFF4100 <= a < 0xFBFF4120: return f"SRAM dev-info record +{a - 0xFBFF4100:#x}"
    if 0xFBFF4000 <= a < 0xFBFF8000: return f"PBS SRAM +{a - 0xFBFF4000:#x}"
    if a >> 32: return "?? (bits 39:32 set; not a documented V2 register)"
    return "?"


def pll_note(name, kind, v):
    if kind == "setup_0":
        nf, nr, od = v & 0x1FFF, (v >> 16) & 0x3F, (v >> 24) & 0xF
        f = REF_HZ * (nf + 1) // ((nr + 1) * (od + 1))
        return f"NF-1={nf} NR-1={nr} OD-1={od}{' relock' if v >> 31 else ''} -> VCO {f / 1e6:.1f} MHz"
    if kind == "setup_1":
        return f"BWADJ-1={v & 0xFFF} lock_count_max={v >> 16:#x}"
    d0, d1 = v & 0x7FF, (v >> 16) & 0x7FF
    fl = [n for b, n in ((12, "relock0"), (13, "last0"), (14, "reset0"), (15, "bypass0"),
                         (28, "relock1"), (29, "last1"), (30, "reset1"), (31, "bypass1")) if v >> b & 1]
    return f"div0={d0} div1={d1} {' '.join(fl)}"


def preload(img):
    out, off, ents = [], 0, []
    if u32(img, 0) != PLD_MAGIC or struct.unpack_from("<Q", img, 4)[0] != PLD_ADDR_MAGIC:
        return None, 0, ["no preload header at 0 (preload absent or not this format)"]
    off = 12
    while off + 12 <= len(img):
        data, addr = struct.unpack_from("<IQ", img, off)
        if data == PLD_MAGIC and addr == PLD_ADDR_MAGIC:
            off += 12
            break
        ents.append((off, addr, data))
        off += 12
    else:
        return ents, off, ["no footer entry"]
    out.append(f"preload table: {len(ents)} writes, footer ends at {off:#x}")
    sram = {}
    for o, a, v in ents:
        n = regname(a)
        note = ""
        for base, pn in PLLS.items():
            if base <= a < base + 0x40:
                k = "setup_0" if a == base else "setup_1" if a == base + 4 else "ch"
                note = pll_note(pn, k, v)
        if 0xFBFF4100 <= a < 0xFBFF4120:
            sram[a - 0xFBFF4100] = v
        out.append(f"  {o:#05x}  [{a:#012x}] <- {v:#010x}  {n}{'  ' + note if note else ''}")
    if sram:
        rec = b"".join(struct.pack("<I", sram.get(i, 0)) for i in range(0, 0x20, 4))
        ver = rec[0x14:0x20].split(b"\0")[0].decode("ascii", "replace")
        ei = rec[0xA] | rec[0xB] << 8
        out.append(f"  SRAM dev-info record: valid byte {rec[0]:#04x}; early-init offset "
                   f"{ei:#06x} (stage2 reads 0x400 when byte 0 is 0); version string '{ver}'")
    trailer = img[off:off + 0x200]
    if len(trailer) == 0x200 and trailer not in (b"\xff" * 0x200, b"\0" * 0x200):
        out.append(f"  trailer {off:#x}-{off + 0x1ff:#x}: 0x200 B, not a byte sum "
                   f"(RSA-2048 sized; unverified, nothing in stage2/3 reads it)")
    return ents, off, out


def early_init(img, base):
    b = img[base:base + 0x40]
    if len(b) < 7 or b[0] != 0xAA:
        return [f"early init @{base:#x}: magic {b[0]:#04x} != 0xaa (stage2: 'invalid early init info!')" if b
                else f"early init @{base:#x}: image too short"]
    a0, o0, a1, o1 = b[1], b[2] | b[3] << 8, b[4], b[5] | b[6] << 8
    nm = lambda a: "preload EEPROM" if a == 0xFF else "none" if a == 0 else f"{a:#04x}"
    out = [f"early init @{base:#x} (magic 0xaa): dimm0 SPD i2c {nm(a0)} offset {o0:#06x}; "
           f"dimm1 SPD i2c {nm(a1)} offset {o1:#06x}"]
    out.append(f"  +0x0b..0x0d = {b[0xB]:02x} {b[0xC]:02x} {b[0xD]:02x} (not decoded)")
    if b[0xE] == 0xCC:
        names = ("1 dimm/1 rank", "1 dimm/2 ranks", "2 dimms/1 rank", "2 dimms/2 ranks")
        out.append(f"  impedance block (magic 0xcc, flag +0x0f={b[0xF]}), stage2 2.44 only; "
                   "[dram_odt dram_odt_dyn dram_rout phy_odt phy_rout]:")
        for i, n in enumerate(names):
            e = b[0x10 + 5 * i:0x15 + 5 * i]
            out.append(f"    {n:16} " + ("unset (ff)" if e == b"\xff" * 5 else " ".join(f"{x:#04x}" for x in e)))
        out.append(f"  +0x24..0x2b = {b[0x24:0x2c].hex(' ')} (not decoded)")
    out.append(f"  +0x2c = {b[0x2c]:#04x} " + ("(0xcd: dual-dimm pu/pd/vref block follows)" if b[0x2c] == 0xCD
                                             else "(!= 0xcd: no pu/pd/vref override; stage2 defaults)"))
    return out, (a0, o0)


def crc16(b):
    c = 0
    for x in b:
        c ^= x << 8
        for _ in range(8):
            c = ((c << 1) ^ 0x1021) & 0xFFFF if c & 0x8000 else (c << 1) & 0xFFFF
    return c


def spd(img, base):
    s = img[base:base + 0x180]
    if len(s) < 128 or s[2] != 0x0C:
        return [f"SPD @{base:#x}: byte 2 = {s[2] if len(s) > 2 else None!r}, not DDR4 (0x0c)"]
    mtb = lambda v, f: v * 125 + (f - 256 if f > 127 else f)
    cols, rows = 9 + (s[5] & 7), 12 + (s[5] >> 3 & 7)
    bg, ba = s[4] >> 6 & 3, 2 + (s[4] >> 4 & 3)
    width = 4 << (s[12] & 7)
    ranks = 1 + (s[12] >> 3 & 7)
    bus = 8 << (s[13] & 7)
    ecc = (s[13] >> 3 & 3) == 1
    dev_bits = rows + cols + bg + ba
    total = (1 << dev_bits) * width // 8 * (bus // width) * ranks
    tck = mtb(s[18], s[125])
    mt = {1: "RDIMM", 2: "UDIMM", 3: "SO-DIMM", 4: "LRDIMM", 0: "extended"}.get(s[3] & 0xF, f"{s[3] & 0xF:#x}")
    ok = crc16(s[:126]) == (s[126] | s[127] << 8)
    out = [f"SPD @{base:#x}: DDR4, module type {mt} (byte3={s[3]:#04x}), bytes0/1 = {s[0]:#04x}/{s[1]:#04x}, "
           f"base CRC {'OK' if ok else 'BAD'}",
           f"  geometry: {rows} row + {cols} col bits, {1 << bg} bank groups x {1 << ba} banks, x{width}, "
           f"{ranks} rank(s), {bus}-bit bus{' + 8-bit ECC' if ecc else ''} -> {total >> 30} GiB "
           f"(density nibble byte4={s[4] & 0xF}: {'consistent' if (256 << (s[4] & 0xF)) == (1 << dev_bits) * width >> 20 else 'INCONSISTENT with geometry'})",
           f"  tCKmin {tck} ps (DDR4-{2 * 10**6 // tck}), tAA {mtb(s[24], s[123])} ps, tRCD {mtb(s[25], s[122])} ps, "
           f"tRP {mtb(s[26], s[121])} ps, tRFC1 {(s[30] | s[31] << 8) * 125 // 1000} ns",
           f"  part number '{s[329 - 0:349].decode('ascii', 'replace').strip() if len(s) >= 349 else ''}'"]
    return out


def board_data(img, show):
    b = img[0x600:0x600 + 0x60]
    if len(b) < 0x60 or b[0] != 0xCC:
        return [f"board data @0x600: magic {b[0]:#04x} != 0xcc (absent; MACs then come from elsewhere)" if b
                else "board data @0x600: image too short"]
    if not show:
        return ["board data @0x600 (magic 0xcc): present; MAC/serial/PN hidden (use --show-ids, keep output private)"]
    mac = lambda m: ":".join(f"{x:02x}" for x in m)
    return [f"board data @0x600: customer_rev {b[1]}, MAC0-3 {[mac(b[2 + 6 * i:8 + 6 * i]) for i in range(4)]}, "
            f"pcb_rev {b[0x1a:0x21]!r}, sn {b[0x21:0x31]!r}, pn {b[0x31:0x40]!r}"]


def strap(v):
    dev = {0: "UART CLI", 1: "UART 2000000", 2: "NAND 8-bit", 4: "UART 115200", 5: "SPI mode 3",
           6: "UART 1000000", 7: "SPI mode 0"}.get(v >> 15 & 7, "?")
    pl = {0: "none", 1: "I2C", 2: "SPI slave", 3: "I2C"}[v >> 12 & 3]
    spd_ = {0: "400K", 1: "750K", 2: "1M", 3: "100K"}[v >> 26 & 3]
    print(f"boot_strap {v:#010x} (V2 layout, HAL al_hal_bootstrap_map.h):")
    print(f"  cpu_pll sel {v & 0xF}, nb_pll sel {v >> 4 & 7}, sb_pll sel {v >> 7 & 3}, sb_clk sel {v >> 9 & 3}")
    print(f"  preload {pl}" + (f" from {0x57 if v >> 23 & 1 else 0x50:#04x} at {spd_}" if pl == "I2C" else ""))
    print(f"  boot_rom bit14={v >> 14 & 1}, boot device {v >> 15 & 7} = {dev}, debug_mode {v >> 18 & 1}, "
          f"pll_ref bit19={v >> 19 & 1}, cpu_exist {v >> 20 & 3}")


def main(a):
    if len(a) >= 2 and a[0] == "--strap":
        return strap(int(a[1], 0))
    if not a or a[0].startswith("-"):
        sys.exit(__doc__)
    img = open(a[0], "rb").read()
    if len(img) < 0x24:
        sys.exit(f"{a[0]}: {len(img)} bytes, too short for a preload image")
    if u32(img, 0) == OBJ_MAGIC:
        oid, size = u32(img, 8), struct.unpack_from("<Q", img, 0x28)[0]
        print(f"al-boot object id {oid:#x}, data {size:#x} B (stage2 accepts EEPROM objects <= 0x400 B, "
              f"multiple of 32, and writes them to EEPROM offset 0)")
        img = img[0x48:0x48 + size]
    ents, end, lines = preload(img)
    print("\n".join(lines))
    ei_off = 0x400
    if ents:
        rec = {a - 0xFBFF4100: v for _, a, v in ents if 0xFBFF4100 <= a < 0xFBFF4120}
        if rec.get(0, 0) & 0xFF and 8 in rec:
            ei_off = rec[8] >> 16 & 0xFFFF
    if len(img) > ei_off:
        r = early_init(img, ei_off)
        if isinstance(r, tuple):
            lines, (sa, so) = r
            print("\n".join(lines))
            if sa == 0xFF and len(img) > so:
                print("\n".join(spd(img, so)))
        else:
            print("\n".join(r))
    if len(img) >= 0x660:
        print("\n".join(board_data(img, "--show-ids" in a)))


if __name__ == "__main__":
    main(sys.argv[1:])
