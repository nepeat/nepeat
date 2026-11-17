#!/usr/bin/env python3
"""Annapurna al-boot, Alpine V3 additions to alboot.py (same directory; imported, not copied):
every TOC the 16-offset search would see, the V3 object names, the non-redundant env, a TOC
builder and in-memory object/BOOT_MODE builders for a second TOC (scheme A',
devices/cs8210-32x/boot-chain.md section 7). Offline only; never touches hardware.

alboot.py itself is unchanged on purpose: the DMC-12 derivations hash it (nix store path), so
any edit would move their pinned drvPaths. The V3 fixes listed in boot-chain.md section 10 live
here instead: all TOCs reported (a TOC2 at 0x20000 shadows 0x40000; one with a bad entry sum is
skipped, as the HAL's al_flash_toc_validate does), V3 names (0x510/0x520/0x530, ...), the
non-redundant env (CRC32 over bytes 4..size), objects beyond the image reported as outside it,
and a header id that differs from its TOC entry is a note (stage3 never compares them).

  alboot3.py list <image.bin[.zst]>          every probe offset, the active TOC, its objects
  alboot3.py toc <image> [--at 0x20000]      one TOC's raw entries (id, name, offset, max)
  alboot3.py wrap --id 0x110 --load 0x08000000 [--entry A] [--version 2026.10.0] in out
  alboot3.py bootmode --app 0x110 out        BOOT_MODE object (0x40 B data, word +4 = app id)

Formats (HAL al_flash_contents.h, stage2/stage3 3.65.1 disassembly): TOC header 0x14 B
{magic 0x070c070c, format_rev_id, num_entries, reserved, byte sum of the first 0x10 B},
entries of 0x20 B {obj_id (instance << 28 | flavour << 16 | id), name[8], dev_id, offset,
max_size, flags, reserved}, then a u32 byte sum over all entry bytes. Object header 0x48 B
{magic 0x000b9ec7, rev, id, maj, min, fix, desc[16], size, load_hi, load_lo, exec_hi, exec_lo,
flags, reserved, byte sum of the first 0x44 B}, data, u32 byte sum of the data.
"""
import pathlib, struct, sys, zlib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import alboot  # noqa: E402

TOC_MAGIC, OBJ_MAGIC, HDR = alboot.TOC_MAGIC, alboot.OBJ_MAGIC, alboot.HDR
TOC_HDR, TOC_ENT = 0x14, 0x20
PROBES = [i * 0x20000 for i in range(16)]       # al_flash_toc_search(…, 0, 0x20000, 16)
NAMES = dict(alboot.NAMES)
NAMES.update({0x13: "UEFI", 0x11: "ATF_BL31", 0x12: "ATF_BL32", 0x210: "APP_0", 0x220: "APP_1",
              0x230: "APP_2", 0x240: "APP_3", 0x300: "CRASH_DUMP", 0x400: "VPD",
              0x510: "SERDES_AVG_PCIE_FW", 0x520: "SERDES_AVG_NON_PCIE_FW",
              0x530: "AVG_SBUS_MASTER_FW", 0x600: "PUK0", 0x601: "PUK1", 0x602: "PUK2",
              0x701: "SPIS_PLD"})
BM_DATA = 0x40            # stage3: BOOT_MODE size > 0x40 -> "Boot mode too big!" -> app 5
bsum, u32 = alboot.bsum, alboot.u32


def name(oid):
    i = oid & 0xFFFF
    return NAMES.get(i, f"USER_{i:#x}" if 0x1000 <= i <= 0x1FFF else f"{i:#x}")


def toc_at(d, o):
    """al_flash_toc_validate at offset o: (None, toc) or (reason, None)."""
    if o + TOC_HDR > len(d):
        return "outside image", None
    if u32(d, o) != TOC_MAGIC:
        st = alboot.state(d[o:o + TOC_HDR])
        return st or f"no magic ({u32(d, o):08x})", None
    if bsum(d[o:o + 0x10]) != u32(d, o + 0x10):
        return "header sum bad", None
    n = u32(d, o + 8)
    end = o + TOC_HDR + n * TOC_ENT + 4
    if end > len(d):
        return f"{n} entries run past the image", None
    if bsum(d[o + TOC_HDR:end - 4]) != u32(d, end - 4):
        return "entries sum bad", None
    ents = []
    for i in range(n):
        e = o + TOC_HDR + i * TOC_ENT
        oid, nm, dev, off, mx, fl, rs = struct.unpack_from("<I8sIIIII", d, e)
        ents.append({"index": i, "obj_id": oid, "id": oid & 0xFFFF, "flavour": (oid >> 16) & 0xFF,
                     "instance": oid >> 28, "name": nm.split(b"\0")[0].decode("ascii", "replace"),
                     "dev": dev, "offset": off, "max": mx, "flags": fl, "reserved": rs,
                     "raw": bytes(d[e:e + TOC_ENT])})
    return None, {"offset": o, "rev": u32(d, o + 4), "n": n, "reserved": u32(d, o + 0xC),
                  "size": end - o, "entries": ents}


def find_tocs(d):
    """Every probe offset with its verdict; the first valid one is what stage2/3 and U-Boot use."""
    out, active = [], None
    for o in PROBES:
        why, t = toc_at(d, o)
        out.append((o, why, t))
        if t and active is None:
            active = t
    return active, out


def find_id(toc, obj_id):
    """al_flash_toc_find_id: first entry whose full obj_id (instance, flavour, id) matches."""
    for e in toc["entries"]:
        if e["obj_id"] == obj_id:
            return e
    return None


def toc_build(entries, rev=0, reserved=0):
    """TOC bytes. entries: 32-byte raw entries (kept verbatim) or dicts with obj_id, name,
    offset, max (dev, flags, reserved default 0)."""
    raw = b""
    for e in entries:
        if isinstance(e, (bytes, bytearray)):
            assert len(e) == TOC_ENT
            raw += bytes(e)
        else:
            nm = e["name"].encode()[:8].ljust(8, b"\0")
            raw += struct.pack("<I8sIIIII", e["obj_id"], nm, e.get("dev", 0), e["offset"],
                               e["max"], e.get("flags", 0), e.get("reserved", 0))
    h = struct.pack("<4I", TOC_MAGIC, rev, len(raw) // TOC_ENT, reserved)
    return h + struct.pack("<I", bsum(h)) + raw + struct.pack("<I", bsum(raw))


def obj_build(data, oid, load=0, exec_=None, version=(1, 0, 0), desc=b"N/A", flags=0):
    """al-boot object (header + data + data sum), addresses below 4 GiB."""
    exec_ = load if exec_ is None else exec_
    assert load < 1 << 32 and exec_ < 1 << 32
    if 0xF3000000 <= load < 0xF3400000:
        raise ValueError("stage3 refuses load addresses in 0xf3000000..0xf33fffff (pre_load_action)")
    h = struct.pack("<6I16s8I", OBJ_MAGIC, 0, oid, *version, bytes(desc)[:15].ljust(16, b"\0"),
                    len(data), 0, load, 0, exec_, flags, 0, 0)[:0x44]
    return h + struct.pack("<I", bsum(h)) + bytes(data) + struct.pack("<I", bsum(data))


def bootmode_build(app_id):
    """BOOT_MODE: struct al_flash_boot_mode {format_rev_id 0, default_app_id, reserved[56]}."""
    return obj_build(struct.pack("<II", 0, app_id).ljust(BM_DATA, b"\0"), 0)


def obj_check(d, o, maxsize=None):
    """stage3's view: al_flash_obj_header_read_and_validate then al_flash_obj_data_load.
    Returns (verdict, header dict or None): 'OK', 'NO_HEADER: why', 'BAD_DATA: why'."""
    if o + HDR > len(d):
        return "NO_HEADER: outside image", None
    f = alboot.obj_header(d, o)
    if isinstance(f, str):
        return "NO_HEADER: " + f, None
    end = o + HDR + f["size"] + 4
    if end > len(d):
        return "BAD_DATA: runs past the image", f
    if bsum(d[o + HDR:o + HDR + f["size"]]) != u32(d, o + HDR + f["size"]):
        return "BAD_DATA: data sum", f
    notes = []
    if maxsize is not None and HDR + f["size"] + 4 > maxsize:
        notes.append(f"larger than its TOC max {maxsize:#x} (stage3 does not check)")
    return "OK" + (" (" + "; ".join(notes) + ")" if notes else ""), f


def env_v3(blob, size=0x4000):
    """Non-redundant U-Boot env: CRC32 (LE) over bytes 4..size."""
    if alboot.state(blob[:size]):
        return alboot.state(blob[:size])
    return "CRC32 OK" if zlib.crc32(blob[4:size]) == u32(blob, 0) else "CRC32 BAD"


def list_image(path):
    d = alboot.load(path)
    print(f"== {path} ({len(d):#x} bytes)")
    active, probes = find_tocs(d)
    for o, why, t in probes:
        if t:
            tag = "ACTIVE" if t is active else "valid, shadowed"
            print(f"  probe {o:#08x}: TOC {t['n']} entries, rev {t['rev']}, {t['size']:#x} B ({tag})")
        elif why not in ("blank(ff)", "zero") and not why.startswith("no magic"):
            print(f"  probe {o:#08x}: {why}")
    if not active:
        print("  no valid TOC: stage2 stops ('al_flash_toc_search failed!')")
        return
    print(f"  {'#':>2} {'obj_id':>10} {'name':22} {'entry':8} {'offset':>8} {'max':>8}  stage3 view")
    for e in active["entries"]:
        off, mx, i = e["offset"], e["max"], e["id"]
        head = (f"  {e['index']:>2} {e['obj_id']:#010x} {name(i) + (' #%d' % e['instance'] if e['instance'] else ''):22}"
                f" {e['name']:8} {off:>8x} {mx:>8x}  ")
        if off >= len(d):
            print(head + "outside this image")
            continue
        if i in alboot.BOOTROM_IDS:
            st, info = alboot.check_s2(d, off, mx)
            print(head + f"S2 image {st}")
            continue
        if i in alboot.ENV_IDS:
            print(head + f"env (non-redundant) {env_v3(d[off:off + 0x4000])}")
            continue
        st, f = obj_check(d, off, mx)
        line = head + st
        if f:
            line += f": v{alboot.vstr(f)} size {f['size']:#x}"
            if i not in alboot.PRIVATE_IDS:
                line += f" load {f['load']:#x} exec {f['exec']:#x}"
            if f["id"] != e["obj_id"]:
                line += f" (header id {f['id']:#x}; stage3 does not compare)"
        print(head[:0] + line)


def main(argv):
    if len(argv) < 2:
        raise SystemExit(__doc__)
    cmd, a = argv[0], argv[1:]
    if cmd == "list":
        for p in a:
            list_image(p)
    elif cmd == "toc":
        at = int(a[a.index("--at") + 1], 0) if "--at" in a else None
        d = alboot.load(a[0])
        t = toc_at(d, at)[1] if at is not None else find_tocs(d)[0]
        if not t:
            raise SystemExit("no valid TOC there")
        for e in t["entries"]:
            print(f"{e['index']:2} {e['obj_id']:#010x} {name(e['id']):22} {e['name']:8} "
                  f"dev {e['dev']} off {e['offset']:#x} max {e['max']:#x} flags {e['flags']:#x}")
    elif cmd == "wrap":
        opts = {"--id": None, "--load": None, "--entry": None, "--version": "1.0.0", "--desc": "N/A"}
        pos, it = [], iter(a)
        for x in it:
            if x in opts:
                opts[x] = next(it)
            else:
                pos.append(x)
        if len(pos) != 2 or None in (opts["--id"], opts["--load"]):
            raise SystemExit(__doc__)
        ld = int(opts["--load"], 0)
        ob = obj_build(open(pos[0], "rb").read(), int(opts["--id"], 0), ld,
                       int(opts["--entry"], 0) if opts["--entry"] else ld,
                       tuple(int(x) for x in opts["--version"].split(".")), opts["--desc"].encode())
        open(pos[1], "wb").write(ob)
        print(f"wrote {pos[1]}: {len(ob):#x} B, crc32 {zlib.crc32(ob):08x}")
    elif cmd == "bootmode":
        app = int(a[a.index("--app") + 1], 0)
        out = [x for x in a if not x.startswith("--") and x != a[a.index("--app") + 1]][0]
        ob = bootmode_build(app)
        open(out, "wb").write(ob)
        print(f"wrote {out}: BOOT_MODE app {app:#x}, {len(ob):#x} B, crc32 {zlib.crc32(ob):08x}")
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
