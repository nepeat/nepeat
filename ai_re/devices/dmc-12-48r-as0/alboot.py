#!/usr/bin/env python3
"""Annapurna al-boot SPI layout: list/verify a flash image, or wrap a binary as an object.

Offline only; never touches hardware. Format and the checks below are the ones
stage2 (v2.60.3/4) and stage3 (2.60.4) do, from their disassembly
(spi-uboot-replacement.md). Every checksum is a plain 32-bit byte sum.

  alboot.py list <mtd.bin[.zst]> [...]      TOC + every object, verified
  alboot.py check <object.fimg>             one object file (header, data sum)
  alboot.py wrap --id 5 --load 0x08000000 [--entry A] [--version 2.60.4] in.bin out.fimg

`list` prints no object contents. EEPROM/env/config objects can hold board
identifiers, so for those it prints only state, size and checksums.
"""
import struct, subprocess, sys

TOC_MAGIC = 0x070C070C
OBJ_MAGIC = 0x000B9EC7
HDR = 0x48
NAMES = {0: "BOOT_MODE", 1: "STG2", 2: "DT", 3: "STG2_5", 4: "STG3", 5: "UBOOT",
         6: "UBOOT_SCRIPT", 7: "UBOOT_ENV", 8: "UBOOT_ENV_RED", 9: "KERNEL",
         0xA: "ROOT_FS", 0xB: "2ND_TOC", 0xC: "PRE_BOOT", 0xD: "PRE_BOOT_V2",
         0x10: "HW_PLD", 0x100: "APCEA", 0x101: "APCEA_CFG", 0x110: "IPXE",
         0x200: "BOOT_APP", 0x201: "BOOT_APP_CFG", 0x500: "SERDES_25G_FW", 0x700: "EEPROM"}
BOOTROM_IDS = (1, 0xC, 0xD)          # objects that start with the boot-ROM "S2" header
ENV_IDS = (7, 8)
PRIVATE_IDS = ENV_IDS + (0x700, 0x101, 0x201)   # may hold MACs/serials/config: no content
ENV_SIZE = 0x4000                    # factory CONFIG_ENV_SIZE (CRC matches only at 16 KiB)


def bsum(b):
    return sum(b) & 0xFFFFFFFF


def load(path):
    if path.endswith(".zst"):
        return subprocess.run(["zstd", "-dc", path], check=True, capture_output=True).stdout
    return open(path, "rb").read()


def u32(d, o):
    return struct.unpack_from("<I", d, o)[0]


def state(blob):
    if blob.count(0xFF) == len(blob):
        return "blank(ff)"
    if blob.count(0) == len(blob):
        return "zero"
    return None


def obj_header(d, o):
    """al-boot object header (0x48 B). Returns dict, or an error string."""
    h = d[o:o + HDR]
    if len(h) < HDR:
        return "short"
    s = state(h[:16])
    if s:
        return s
    if u32(h, 0) != OBJ_MAGIC:
        return f"bad magic {u32(h, 0):08x}"
    if bsum(h[:0x44]) != u32(h, 0x44):
        return f"bad header sum {bsum(h[:0x44]):x} != {u32(h, 0x44):x}"
    f = dict(zip(("magic", "rsvd", "id", "vmaj", "vmin", "vsub"), struct.unpack_from("<6I", h)))
    f["desc"] = h[0x18:0x28].split(b"\0")[0].decode("ascii", "replace")
    f["size"], f["load"], f["exec"] = struct.unpack_from("<3Q", h, 0x28)
    f["flags"], f["hsum"] = struct.unpack_from("<2I", h, 0x40)
    return f


def check_object(d, o, maxsize, want_id=None):
    """As stage3 al_flash_obj_header_read_and_validate + al_flash_obj_data_load."""
    f = obj_header(d, o)
    if isinstance(f, str):
        return f, None
    errs = []
    if want_id is not None and f["id"] != want_id:
        errs.append(f"id {f['id']:x} != TOC {want_id:x}")
    end = HDR + f["size"] + 4
    if maxsize and end > maxsize:
        errs.append(f"size {end:#x} > slot {maxsize:#x}")
    if o + end > len(d):
        errs.append("truncated")
    else:
        dsum = bsum(d[o + HDR:o + HDR + f["size"]])
        stored = u32(d, o + HDR + f["size"])
        f["dsum"] = dsum
        if dsum != stored:
            errs.append(f"data sum {dsum:x} != {stored:x}")
    return ("BAD " + "; ".join(errs)) if errs else "OK", f


def check_s2(d, o, maxsize):
    """Boot-ROM stage2 image: 'S2' header (0x20 B, u16 byte sum at 0x1e over 0..0x1d),
    payload, u16 payload byte sum, optional 0x200 B RSA-2048 blob (key | signature),
    then an al-boot STG2 header at the end whose size = bytes before it (no data sum)."""
    h = d[o:o + 0x20]
    s = state(h)
    if s:
        return s, {}
    if h[:2] != b"S2":
        return f"no S2 magic ({h[:4].hex()})", {}
    w = struct.unpack("<8I", h)
    errs, info = [], {"payload": w[3], "exec": w[4], "extra": (w[1], w[2], w[5], w[6])}
    if (sum(h[:0x1E]) & 0xFFFF) != struct.unpack_from("<H", h, 0x1E)[0]:
        errs.append("S2 header sum")
    p_end = o + 0x20 + w[3]
    psum = sum(d[o + 0x20:p_end]) & 0xFFFF
    if psum != struct.unpack_from("<H", d, p_end)[0]:
        errs.append("S2 payload sum")
    info["psum"] = psum
    for blob in (0x200, 0):              # signed images carry a 0x200 B blob first
        a = p_end + 2 + blob
        if u32(d, a) == OBJ_MAGIC:
            info["sig"] = blob
            st, f = obj_header(d, a), None
            if isinstance(st, str):
                errs.append("STG2 header " + st)
            else:
                f = st
                if f["size"] != a - o:
                    errs.append(f"STG2 header size {f['size']:#x} != {a - o:#x}")
                info["hdr"] = f
            break
    else:
        errs.append("no al-boot STG2 header after payload")
    if maxsize and "hdr" in info and info["hdr"]["size"] + HDR > maxsize:
        errs.append("exceeds slot")
    return ("BAD " + "; ".join(errs)) if errs else "OK", info


def find_toc(d):
    """stage2/3 al_flash_toc_search: 16 tries, 128 KiB apart, from offset 0."""
    for i in range(16):
        o = i * 0x20000
        if u32(d, o) != TOC_MAGIC:
            continue
        if bsum(d[o:o + 0x10]) != u32(d, o + 0x10):
            print(f"  TOC @{o:#x}: header sum bad")
            continue
        n = u32(d, o + 8)
        ents = d[o + 0x14:o + 0x14 + n * 0x20]
        ok = bsum(ents) == u32(d, o + 0x14 + n * 0x20)
        return o, n, ok
    return None, 0, False


def vstr(f):
    return f"{f['vmaj']}.{f['vmin']}.{f['vsub']}"


def list_image(path):
    d = load(path)
    print(f"== {path} ({len(d):#x} bytes)")
    toc, n, ok = find_toc(d)
    if toc is None:
        print("  no TOC found (stage3 would fall back to raw offsets 0x40000/0xc0000)")
        return
    print(f"  TOC @{toc:#x}: magic {TOC_MAGIC:08x}, header sum OK, {n} entries, "
          f"entries sum {'OK' if ok else 'BAD'}")
    print(f"  {'#':>2} {'id':>8} {'inst':>4} {'name':8} {'offset':>8} {'max':>8}  state / header")
    for i in range(n):
        e = toc + 0x14 + i * 0x20
        eid, name, dev, off, mx, r18, r1c = struct.unpack_from("<I8sIIIII", d, e)
        oid, inst = eid & 0x0FFFFFFF, eid >> 28
        nm = NAMES.get(oid, f"{oid:#x}")
        head = f"  {i:>2} {oid:>#8x} {inst:>4} {name.split(b'\0')[0].decode():8} {off:>8x} {mx:>8x}  "
        if dev or r18 or r1c:
            head += f"[dev {dev} r18 {r18:x} r1c {r1c:x}] "
        if oid in BOOTROM_IDS:
            st, info = check_s2(d, off, mx)
            line = f"{nm} {st}"
            if "payload" in info:
                line += f"; S2 payload {info['payload']:#x} exec {info['exec']:#x}"
                if any(info["extra"]):
                    line += f" extra {info['extra']}"
                if "sig" in info:
                    line += "; RSA blob " + ("0x200 B" if info["sig"] else "absent")
                if "hdr" in info:
                    line += f"; al-boot hdr v{vstr(info['hdr'])} size {info['hdr']['size']:#x}"
            print(head + line)
            continue
        if oid in ENV_IDS:
            blob = d[off:off + ENV_SIZE]
            s = state(blob)
            if s:
                print(head + f"{nm} {s}")
                continue
            import zlib
            good = zlib.crc32(blob[5:]) == u32(blob, 0)
            print(head + f"{nm} raw U-Boot env, {ENV_SIZE:#x} B, CRC32 {'OK' if good else 'BAD'}, "
                  f"redundancy flag {blob[4]}")
            continue
        st, f = check_object(d, off, mx, oid)
        if f is None:
            print(head + f"{nm} {st}")
            continue
        line = f"{nm} {st}: v{vstr(f)} size {f['size']:#x}"
        if oid not in PRIVATE_IDS:
            line += f" load {f['load']:#x} exec {f['exec']:#x} flags {f['flags']:#x} desc {f['desc']!r}"
        line += f" hsum {f['hsum']:#x} dsum {f.get('dsum', 0):#x}"
        print(head + line)
    # Stage2 copies the boot ROM might find that the TOC doesn't list.
    listed = {u32(d, toc + 0x14 + i * 0x20 + 0x10) for i in range(n)}
    extra = [o for o in range(0, 0x100000, 0x20000) if d[o:o + 2] == b"S2" and o not in listed]
    for o in extra:
        st, info = check_s2(d, o, 0x20000)
        print(f"  (not in TOC) S2 image @{o:#x}: {st}, payload {info.get('payload', 0):#x}")


def check_file(path):
    d = load(path)
    if d[:2] == b"S2":
        st, info = check_s2(d, 0, 0)
    else:
        st, info = check_object(d, 0, 0)
    print(f"{path}: {st}")
    if isinstance(info, dict) and "size" in info:
        print(f"  id {info['id']:#x} ({NAMES.get(info['id'], '?')}) v{vstr(info)} size {info['size']:#x} "
              f"load {info['load']:#x} exec {info['exec']:#x} flags {info['flags']:#x}")
    return st == "OK"


def wrap(argv):
    opts = {"--id": "5", "--load": None, "--entry": None, "--version": "2.60.4", "--desc": "N/A"}
    pos = []
    it = iter(argv)
    for a in it:
        if a in opts:
            opts[a] = next(it)
        else:
            pos.append(a)
    if len(pos) != 2 or opts["--load"] is None:
        raise SystemExit(__doc__)
    src, dst = pos
    oid, ld = int(opts["--id"], 0), int(opts["--load"], 0)
    ent = int(opts["--entry"], 0) if opts["--entry"] else ld
    vmaj, vmin, vsub = (int(x) for x in opts["--version"].split("."))
    if 0xF2200000 <= ld < 0xF2300000:
        raise SystemExit("stage3 refuses load addresses in 0xf2200000..0xf22fffff (pre_load_action)")
    data = open(src, "rb").read()
    desc = opts["--desc"].encode()[:15].ljust(16, b"\0")
    h = struct.pack("<6I16s3QI", OBJ_MAGIC, 0, oid, vmaj, vmin, vsub, desc, len(data), ld, ent, 0)
    h += struct.pack("<I", bsum(h))
    img = h + data + struct.pack("<I", bsum(data))
    slot = {5: 0xE0000, 4: 0x70000, 2: 0x10000}.get(oid)
    if slot and len(img) > slot:
        raise SystemExit(f"{len(img):#x} B does not fit the {NAMES[oid]} slot ({slot:#x})")
    open(dst, "wb").write(img)
    print(f"wrote {dst}: {len(img):#x} B, id {oid:#x}, load {ld:#x}, exec {ent:#x}, "
          f"hsum {u32(h, 0x44):#x}, dsum {bsum(data):#x}")


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "list":
        for p in args:
            list_image(p)
    elif cmd == "check":
        sys.exit(0 if all(check_file(p) for p in args) else 1)
    elif cmd == "wrap":
        wrap(args)
    else:
        raise SystemExit(__doc__)
