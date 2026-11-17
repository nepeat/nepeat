#!/usr/bin/env python3
"""CS8210-32X: put our U-Boot in the boot SPI flash under scheme A' (boot-chain.md section 7) and
take it out again. Plan, gates and failure handling: ../spi-upgrade-plan.md.

What it writes, and nothing else (the installer U-Boot cannot write anything else either, 0027/0028):
  0x020000 +0x1000   TOC2: the 9 factory entries verbatim + BOOT_MODE + IPXE 0 + IPXE 1
  0x030000 +0x1000   BOOT_MODE (0x40 B data, word +4 = app: 0x110 = IPXE 0, 0x10000110 = IPXE 1)
  0x0f0000 +0xa0000  slot A = IPXE 0: our U-Boot as an al-boot object, load = exec = 0x08000000
  0x2f9000 +0x67000  slot B = IPXE 1 (updates go to the inactive slot, then BOOT_MODE flips)
Factory TOC (0x40000), STG2/STG3/DT/FW, UBOOT 0 (the "u"-key escape), env and ONIE stay untouched.

Modes (offline unless noted):
  build    --payload u-boot.bin       the exact region bytes + the modified image (images/spi-upgrade/)
  verify   --payload ...              bootchain_emu.py on the result, every cut point of the write
                                      sequence and the traps; records a 'verify' journal entry
  rehearse --payload ... --uboot SANDBOX   the same command sequence through the installer's
                                      sandbox twin (#uboot-sandbox-cs8210-installer) on a file copy
                                      of the flash, cut after every destructive command, each state
                                      emulated; records a 'rehearse' entry
  status                              the model (dump + journal) and what stage3 would boot from it
  linux-check   [hardware, read-only] NixOS /dev/mtd0ro == the model
  install|update|switch|rollback [hardware]  vendor prompt -> loadx #uboot-cs8210-installer -> booti
                                      -> writes; without --write: every read-only step (full flash
                                      read vs the model, payload upload + crc) runs and the writes
                                      are only printed, then a power cycle back to the normal boot.

Safety (spi-upgrade-plan.md): host model of mtd0 = the verified 11-15 dump + journaled region
writes (intent before, done after a matching read-back); a full read must equal the model before
any write. Destructive commands only at the "cs8210-installer> " prompt, typed, echo compared
character for character, then Enter (Ctrl-C otherwise); only the four regions above. Objects are
written body first, header last, each step read back (crc32 + cmp.b); the TOC2 last. A failed
BOOT_MODE or TOC2 step erases that sector again at once. Never saveenv / env default anywhere.
Hardware runs need root's ssh key (consoled, root@cs8210): run with sudo or --ssh 'sudo -n ssh'.
"""
import argparse, hashlib, json, os, pathlib, pty, re, select, shlex, struct, subprocess, sys, time, uuid, zlib

ROOT = pathlib.Path(__file__).resolve().parent
DEV = ROOT.parent
AI = DEV.parents[1]
sys.path[:0] = [str(ROOT), str(AI / "nix/packages/alboot")]
import alboot3 as a3          # noqa: E402
import uart_recovery as ur    # noqa: E402

BASE_MTD0 = DEV / "images/spi-2026-11-15/mtd0-uboot.bin"
BASE_SHA = "a22ff482563b84bd69a70c7b3c0f6eeb67cc73f2d03af89157e996f8681897f6"
FULL_IMAGE = DEV / "analysis/boot-chain/spi0-full-2026-11-15.bin"    # 32 MiB, for the emulation
MODEL_LEN = 0x380000              # mtd0; the env (mtd1) changes with every saveenv: not modelled
TOC2, BM, SECTOR, FACTORY_TOC = 0x20000, 0x30000, 0x1000, 0x40000
SLOTS = {0: (0x0F0000, 0xA0000), 1: (0x2F9000, 0x67000)}
APP = {0: 0x110, 1: 0x10000110}   # IPXE instance n (stage3 ORs instance << 28 = 0: kept)
WINDOWS = sorted([(TOC2, SECTOR), (BM, SECTOR), *SLOTS.values()])
FACTORY_IDS = [0x1, 0x2, 0x510, 0x520, 0x530, 0x4, 0x5, 0x7, 0x8]
LOAD = 0x08000000
UBOOT0 = 0x190000
A_BUNDLE, A_READ, A_RB = 0x04400000, 0x05000000, 0x05800000     # installer RAM staging
JOURNAL = DEV / "spi-upgrade-journal.jsonl"
WRITES = DEV / "images/spi-upgrade"
GATES = DEV / "spi-upgrade-gates.json"
GATE_NAMES = ("review", "payload", "owner_go")
INSTALLER_PROMPT, VENDOR_PROMPT = "cs8210-installer> ", "ALPINE_DB> "
CONSOLE, PORT, HOST, PLUG = "console@10.60.2.10", "cs8210", "root@10.60.2.190", "1"
DONT_POWER_OFF = ("do NOT power off: the installer is still running; rollback --write from its prompt "
                  "(erase 0x20000) or re-run the step")


class Fail(Exception):
    pass


def sha(b):
    return hashlib.sha256(bytes(b)).hexdigest()


def say(msg):
    print(f"[spi-upgrade] {msg}", flush=True)


# ---------------------------------------------------------------- journal + model
def journal(path=JOURNAL):
    out = []
    if path.exists():
        lines = path.read_text().splitlines()
        for i, l in enumerate(lines):
            if not l.strip():
                continue
            try:
                out.append(json.loads(l))
            except ValueError:
                if i == len(lines) - 1:      # torn last line: its command never went out
                    continue
                raise Fail(f"{path}: line {i + 1} is not JSON")
    return out


def log(rec, path=JOURNAL):
    rec = {"t": time.strftime("%Y-%m-%dT%H:%M:%S%z"), **rec}
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as f:
        f.write(json.dumps(rec) + "\n")
        f.flush()
        os.fsync(f.fileno())             # an intent is on disk before its command goes out
    return rec


def keep(data):
    WRITES.mkdir(parents=True, exist_ok=True)
    p = WRITES / f"region-{sha(data)[:16]}.bin"
    p.write_bytes(data)
    return str(p.relative_to(DEV))


def base_image():
    d = BASE_MTD0.read_bytes()
    if sha(d) != BASE_SHA:
        raise Fail(f"{BASE_MTD0} is not the verified 2026-11-15 mtd0 dump")
    return bytearray(d[:MODEL_LEN])


def model(path=JOURNAL):
    """(expected mtd0, unknown ranges): the dump with every verified region write applied."""
    img, unknown = base_image(), []
    recs = journal(path)
    done = {r["id"] for r in recs if r.get("kind") == "write" and r.get("phase") == "done"}
    for r in recs:
        if r.get("kind") != "write" or r.get("phase") != "intent":
            continue
        lo, n = r["off"], r["len"]
        f = DEV / r["file"]
        data = f.read_bytes() if f.exists() else None
        if r["id"] not in done or data is None or sha(data) != r["sha256"]:
            unknown.append((lo, lo + n))
            continue
        img[lo:lo + n] = data
        unknown = [u for u in unknown if not (lo <= u[0] and u[1] <= lo + n)]
    return img, unknown


def segments(unknown, end=MODEL_LEN):
    out, lo = [], 0
    for u0, u1 in sorted(unknown):
        if u0 > lo:
            out.append((lo, u0))
        lo = max(lo, u1)
    return out + ([(lo, end)] if lo < end else [])


def overlaps(a0, a1, b0, b1):
    return a0 < b1 and b0 < a1


# ---------------------------------------------------------------- layout and plan
def factory_toc(img):
    why, t = a3.toc_at(img, FACTORY_TOC)
    if not t or [e["obj_id"] for e in t["entries"]] != FACTORY_IDS:
        raise Fail(f"factory TOC at 0x40000 is not the 9-entry layout this tool knows ({why})")
    return t


def check_layout(img):
    """Nothing of ours overlaps a factory object's bytes or an orphaned reference object."""
    t = factory_toc(img)
    for e in t["entries"]:
        if e["id"] in (7, 8):
            continue
        st, f = a3.obj_check(img, e["offset"]) if e["id"] != 1 else ("S2", None)
        end = e["offset"] + (a3.HDR + f["size"] + 4 if f else 0x1B0E3)
        for lo, n in WINDOWS:
            if overlaps(lo, lo + n, e["offset"], end):
                raise Fail(f"window {lo:#x}+{n:#x} overlaps factory {a3.name(e['id'])} data ({e['offset']:#x}..{end:#x})")
    for lo, n in WINDOWS:          # the orphans in UBOOT's max (boot-chain.md section 1) stay outside
        if overlaps(lo, lo + n, 0x278090, 0x2F8E4C) or overlaps(lo, lo + n, 0x360000, 0x380000):
            raise Fail(f"window {lo:#x}+{n:#x} overlaps an orphaned reference object")


def payload_obj(path, inst):
    data = pathlib.Path(path).read_bytes()
    if data[0x38:0x3C] != b"ARM\x64" or struct.unpack_from("<Q", data, 8)[0] != LOAD:
        raise Fail(f"{path}: not an AArch64 Image-header U-Boot linked at {LOAD:#x}")
    ver = re.search(rb"U-Boot 2026\.\d\d[\w.-]*", data)
    obj = a3.obj_build(data, 0x110, LOAD, LOAD, (2026, 10, 0), b"U-Boot 2026.10")
    if len(obj) > SLOTS[inst][1]:
        raise Fail(f"payload object {len(obj):#x} B does not fit slot {'AB'[inst]} ({SLOTS[inst][1]:#x})")
    return obj, (ver.group().decode() if ver else "?"), data


def toc2_build(img):
    t = factory_toc(img)
    new = [{"obj_id": 0, "name": "bootmode", "offset": BM, "max": SECTOR},
           {"obj_id": APP[0], "name": "ipxe", "offset": SLOTS[0][0], "max": SLOTS[0][1]},
           {"obj_id": APP[1], "name": "ipxe", "offset": SLOTS[1][0], "max": SLOTS[1][1]}]
    return a3.toc_build([e["raw"] for e in t["entries"]] + new, t["rev"], t["reserved"])


def region(name, off, length, obj, split):
    if (off, length) not in WINDOWS:
        raise Fail(f"refusing region {off:#x}+{length:#x}: not one of the scheme A' windows")
    if len(obj) > length:
        raise Fail(f"{name}: {len(obj):#x} B > region {length:#x}")
    return {"name": name, "off": off, "len": length, "obj": bytes(obj), "split": split,
            "want": bytes(obj) + b"\xff" * (length - len(obj))}


def boot_mode_app(img):
    """What stage3 makes of the model's BOOT_MODE (only if TOC2 is valid): app id, 5 for the
    factory U-Boot, or None for the hang (valid header, bad data)."""
    st, f = a3.obj_check(img, BM)
    if st.startswith("NO_HEADER") or (f and f["size"] > a3.BM_DATA):
        return 5
    if st.startswith("BAD_DATA"):
        return None
    return struct.unpack_from("<I", img, BM + a3.HDR + 4)[0]


def plan(mode, img, payload=None, slot=None):
    """Ordered regions for a mode. install: slot A, BOOT_MODE, TOC2 last. update: the inactive
    slot, then BOOT_MODE. switch: BOOT_MODE. rollback: erase TOC2."""
    check_layout(img)
    toc_live = a3.toc_at(img, TOC2)[1] is not None
    regs, info = [], {}
    if mode == "install":
        if toc_live or any(img[o:o + n] != b"\xff" * n for o, n in WINDOWS if o != SLOTS[1][0]):
            raise Fail("install needs TOC2, BOOT_MODE and slot A blank (use update/rollback)")
        inst = 0 if slot is None else slot
        obj, ver, _ = payload_obj(payload, inst)
        regs = [region("app", *SLOTS[inst], obj, a3.HDR),
                region("bootmode", BM, SECTOR, a3.bootmode_build(APP[inst]), a3.HDR),
                region("toc2", TOC2, SECTOR, toc2_build(img), a3.TOC_HDR)]
        info = {"slot": "AB"[inst], "app": hex(APP[inst]), "payload_version": ver}
    elif mode in ("update", "switch"):
        if not toc_live or bytes(img[TOC2:TOC2 + SECTOR]) != toc2_build(img).ljust(SECTOR, b"\xff"):
            raise Fail("TOC2 is not the one this tool writes: install first")
        cur = boot_mode_app(img)
        if cur not in APP.values():
            raise Fail(f"BOOT_MODE selects {cur!r}, not one of our slots: rollback, then install")
        active = 0 if cur == APP[0] else 1
        inst = (1 - active) if slot is None else slot
        if inst == active:
            raise Fail(f"slot {'AB'[inst]} is the active one; never rewritten in place")
        if mode == "update":
            obj, ver, _ = payload_obj(payload, inst)
            regs.append(region("app", *SLOTS[inst], obj, a3.HDR))
            info["payload_version"] = ver
        else:
            st, _ = a3.obj_check(img, SLOTS[inst][0], SLOTS[inst][1])
            if st != "OK":
                raise Fail(f"slot {'AB'[inst]} holds no valid object ({st}): update it first")
        regs.append(region("bootmode", BM, SECTOR, a3.bootmode_build(APP[inst]), a3.HDR))
        info.update(slot="AB"[inst], app=hex(APP[inst]))
    elif mode == "rollback":
        regs = [region("toc2-erase", TOC2, SECTOR, b"", 0)]
    else:
        raise Fail(mode)
    after = bytearray(img)
    for r in regs:
        after[r["off"]:r["off"] + r["len"]] = r["want"]
    key = sha(json.dumps([[r["off"], r["len"], sha(r["want"])] for r in regs]).encode())
    return {"mode": mode, "regions": regs, "after": after, "plan_sha": key, **info}


def plan_summary(p):
    return {"mode": p["mode"], "plan_sha": p["plan_sha"][:16],
            **{k: p[k] for k in ("slot", "app", "payload_version") if k in p},
            "regions": [f"{r['name']} {r['off']:#x}+{r['len']:#x} obj {len(r['obj']):#x} B sha256 {sha(r['obj'])[:16]} "
                        f"region crc32 {zlib.crc32(r['want']):08x}" for r in p["regions"]]}


def full_image(mtd0):
    d = bytearray(FULL_IMAGE.read_bytes())
    if sha(d[:MODEL_LEN]) != sha(base_image()):
        raise Fail(f"{FULL_IMAGE} does not start with the verified mtd0")
    d[:MODEL_LEN] = mtd0
    return d


def write_build(p):
    out = WRITES / p["plan_sha"][:16]
    out.mkdir(parents=True, exist_ok=True)
    for r in p["regions"]:
        (out / f"{r['name']}.region").write_bytes(r["want"])
        if r["obj"]:
            (out / f"{r['name']}.obj").write_bytes(r["obj"])
    (out / "mtd0-after.bin").write_bytes(p["after"])
    (out / "plan.json").write_text(json.dumps(plan_summary(p), indent=1) + "\n")
    return out


# ---------------------------------------------------------------- verification (emulation)
def classify(r, payload_sha):
    if "HANG" in r["verdict"]:
        return "hang"
    s3 = r["stage3"]
    if s3["app_off"] == UBOOT0 and s3["load"] == 0x01100000 and s3["app_id"] == 5:
        return "factory"
    for n, (o, _) in SLOTS.items():
        if s3["app_off"] == o and s3["load"] == LOAD and s3["data_sha"] == payload_sha:
            return f"ours:{'AB'[n]}"
    return "other: " + r["verdict"]


def cut_states(img, regs):
    """Flash states a power cut can leave while regs are written in order (body, then header):
    per region erase half done, erased, body half / fully programmed, header partly programmed
    (random subsets of its 0 bits, as a cut mid page-program can leave), complete."""
    import random
    rnd = random.Random(8210)
    cur, out = bytearray(img), []
    for r in regs:
        o, n, w, s = r["off"], r["len"], r["want"], r["split"]
        half = bytearray(cur)
        h = max(SECTOR, n // 2 // SECTOR * SECTOR)
        half[o:o + h] = b"\xff" * h
        if n > SECTOR:
            out.append((f"{r['name']}: erase cut half-way", bytes(half)))
        cur[o:o + n] = b"\xff" * n
        out.append((f"{r['name']}: erased", bytes(cur)))
        if not r["obj"]:
            continue
        body = len(r["obj"]) - s
        mid = bytearray(cur)
        k = s + (body // 2) // 256 * 256
        mid[o + s:o + k] = w[s:k]
        if k > s:
            out.append((f"{r['name']}: body cut half-way", bytes(mid)))
        cur[o + s:o + len(r["obj"])] = w[s:len(r["obj"])]
        out.append((f"{r['name']}: body done, header blank", bytes(cur)))
        for i in range(3):
            t = bytearray(cur)
            for j in range(s):
                made = rnd.getrandbits(8)            # which of the target's 0 bits got programmed
                t[o + j] = 0xFF & ~((~w[j] & 0xFF) & made)
            out.append((f"{r['name']}: header cut (random partial program {i})", bytes(t)))
        cur[o:o + s] = w[:s]
        out.append((f"{r['name']}: complete", bytes(cur)))
    return out


def _boot(args):
    import bootchain_emu as be
    img, keys, ram = args
    return be.boot(full_image(img), keys, ram)


def emulate(cases, jobs=4):
    from concurrent.futures import ProcessPoolExecutor
    with ProcessPoolExecutor(jobs) as ex:
        return list(ex.map(_boot, [(img, keys, ram) for _, img, keys, ram, _ in cases]))


def mode_verify(a, img, unknown):
    if unknown:
        raise Fail(f"model has unknown ranges {unknown}")
    slot = a.slot or 0
    p = plan("install", img, a.payload, slot)
    payload_sha = sha(p["regions"][0]["obj"][a3.HDR:-4])
    inst = p["after"]
    us = "ours:" + p["slot"]
    cases = [("factory dump", bytes(img), b"", False, {"factory"}),
             ("factory + key p (on hw 11-15: 'User request for skipping APCEA!')", bytes(img), b"p" * 8, False,
              {"factory"}),
             ("installed", bytes(inst), b"", False, {us}),
             ("installed + key u", bytes(inst), b"u" * 8, False, {"factory"}),
             ("installed + key p", bytes(inst), b"p" * 8, False, {"factory"}),
             ("installed, Route A RAM reader", bytes(inst), b"", True, {us}),
             ("factory, Route A RAM reader", bytes(img), b"", True, {"factory"})]
    for name, st in cut_states(img, p["regions"]):
        cases.append(("install cut: " + name, st, b"", False, {"factory", us}))
        cases.append(("install cut: " + name + " + key u", st, b"u" * 8, False, {"factory"}))
    other = 1 - slot
    pu = plan("update", inst, a.payload, other)
    cases.append(("updated (other slot + BOOT_MODE flip)", bytes(pu["after"]), b"", False, {"ours:" + "AB"[other]}))
    for name, st in cut_states(inst, pu["regions"]):
        cases.append(("update cut: " + name, st, b"", False, {"factory", "ours:A", "ours:B"}))
    pr = plan("rollback", inst)
    cases.append(("rolled back (TOC2 erased)", bytes(pr["after"]), b"", False, {"factory"}))
    for name, st in cut_states(inst, pr["regions"]):
        cases.append(("rollback cut: " + name, st, b"", False, {"factory", us}))
    # the traps: unreachable with the write order above, shown so its reason is on record
    t = bytearray(inst)
    t[BM + a3.HDR + 4] ^= 0x01                                   # BOOT_MODE data sum broken
    cases += [("TRAP: BOOT_MODE valid header, bad data", bytes(t), b"", False, {"hang"}),
              ("TRAP: same + key u (no escape)", bytes(t), b"u" * 8, False, {"hang"})]
    t = bytearray(inst)
    t[SLOTS[slot][0] + 0x1000] ^= 0xFF                           # app data broken, header valid
    cases += [("app valid header, bad data", bytes(t), b"", False, {"hang"}),
              ("app valid header, bad data + key u", bytes(t), b"u" * 8, False, {"factory"})]
    t = bytearray(inst)
    t[BM:BM + SECTOR] = a3.bootmode_build(APP[other]).ljust(SECTOR, b"\xff")   # the blank slot
    cases += [("BOOT_MODE -> blank slot", bytes(t), b"", False, {"hang"}),
              ("BOOT_MODE -> blank slot + key u", bytes(t), b"u" * 8, False, {"factory"})]
    t = bytearray(inst)
    big = a3.obj_build(struct.pack("<II", 0, APP[slot]).ljust(0x44, b"\0"), 0)
    t[BM:BM + SECTOR] = big.ljust(SECTOR, b"\xff")
    cases.append(("BOOT_MODE data 0x44 B (too big)", bytes(t), b"", False, {"factory"}))
    t = bytearray(inst)
    t[TOC2 + a3.TOC_HDR + 11 * a3.TOC_ENT] ^= 0x01                # TOC2 entries sum broken
    cases.append(("TOC2 entries sum broken", bytes(t), b"", False, {"factory"}))
    say(f"emulating {len(cases)} flash states (stage2 + stage3 3.65.1 in Unicorn) ...")
    res = emulate(cases, a.jobs)
    lines, bad = [], 0
    for (name, _, keys, ram, want), r in zip(cases, res):
        got = classify(r, payload_sha)
        ok = got in want
        bad += not ok
        extra = ""
        if "stage3" in r:
            pr3 = [t for lvl, t in r["stage3"]["prints"] if (lvl <= 2 or "User request" in t or "too big" in t)
                   and "not found in TOC" not in t and "cpu_resume" not in t]
            if pr3:
                extra = "  | " + "; ".join(pr3)[:160]
        lines.append(f"{'ok ' if ok else 'BAD'} {name:64s} -> {got:8s}{extra}")
    ix = [c[0] for c in cases].index("installed")
    s2f, s2i = res[0]["stage2"], res[ix]["stage2"]
    head = [f"bootchain_emu verify {time.strftime('%Y-%m-%d %H:%M %Z')}: plan {p['plan_sha'][:16]} (install slot {p['slot']}),",
            f"  payload {p['payload_version']} data sha256 {payload_sha[:16]}, base mtd0 {BASE_SHA[:16]}",
            f"  stage2: {s2i['stop']}; measured-boot extends factory {len(s2f['extends'])}, installed {len(s2i['extends'])};"
            f" MMIO pages touched {[hex(x) for x in s2i['auto']]}",
            "  installed, stage3 prints: " + " | ".join(t for _, t in res[ix]["stage3"]["prints"])[:700],
            "  expected: 'ours'/'factory' = what stage3 executes (by flash offset + data hash); hang = stage3 or stage2 stops"]
    text = "\n".join(head + lines + [f"{len(cases) - bad}/{len(cases)} as expected"]) + "\n"
    print(text)
    if a.out:
        pathlib.Path(a.out).write_text(text)
    if bad:
        raise Fail(f"{bad} flash states not as expected")
    log({"kind": "verify", "plan_sha": p["plan_sha"], "cases": len(cases), "pass": True})
    return p


# ---------------------------------------------------------------- console
ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")


class PtyProc:
    """A child (the sandbox U-Boot) on a pty: the read/write interface of ur.Tty/ur.Pipe."""
    def __init__(self, argv, cwd):
        import tty
        m, s = pty.openpty()
        tty.setraw(s)                          # 8-bit clean: no ISIG/IXON/ICRNL on XMODEM data
        self.p = subprocess.Popen(argv, stdin=s, stdout=s, stderr=s, cwd=cwd, close_fds=True)
        os.close(s)
        self.rfd = self.wfd = m
        os.set_blocking(m, False)

    def write(self, b):
        for i in range(0, len(b), 16):           # the sandbox console drops bytes in big bursts
            c = b[i:i + 16]
            while c:
                try:
                    n = os.write(self.wfd, c)
                except BlockingIOError:
                    select.select([], [self.wfd], [], 1)
                    continue
                c = c[n:]
            if len(b) > 16:
                time.sleep(0.002)

    def kill(self):
        if self.p.poll() is None:
            self.p.kill()
            self.p.wait()

    def close(self):
        self.kill()
        try:
            os.close(self.rfd)
        except OSError:
            pass


class Console:
    def __init__(self, t, transcript):
        self.t, self.buf, self.f = t, "", open(transcript, "a")

    def pump(self, timeout):
        try:
            b = ur.read_some(self.t, timeout)
        except OSError:
            b = b""
        if b:
            s = ANSI.sub("", b.decode("latin-1")).replace("\r", "")
            self.buf += s
            self.f.write(s)
            self.f.flush()
        return bool(b)

    def mark(self):
        self.buf = ""

    def expect(self, pattern, timeout, fail=None):
        end, rx = time.monotonic() + timeout, re.compile(pattern)
        while time.monotonic() < end:
            if rx.search(self.buf):
                return self.buf
            if fail and re.search(fail, self.buf):
                raise Fail(f"saw /{fail}/ waiting for /{pattern}/: ...{self.buf[-300:]!r}")
            self.pump(0.1)
        raise Fail(f"no /{pattern}/ within {timeout}s: ...{self.buf[-300:]!r}")

    def send(self, text):
        self.f.write(f"\n[>> {text!r}]\n")
        self.f.flush()
        self.t.write(text.encode())

    def sync(self, prompt):
        """An idle prompt: a harmless echo (never a bare Enter: U-Boot repeats the last command,
        e.g. an "sf erase", on an empty line), then a fresh prompt line after its output."""
        for _ in range(3):
            self.n = getattr(self, "n", 0) + 1
            tag = f"sync-{os.getpid()}-{self.n}"
            self.mark()
            self.send(f"echo {tag}\r")
            end = time.monotonic() + 5
            while time.monotonic() < end:
                self.pump(0.1)
                if re.search(r"(^|\n)" + re.escape(tag) + r"\n" + re.escape(prompt) + "$", self.buf):
                    return
            self.send("\x03")
        raise Fail(f"no idle {prompt!r} prompt: ...{self.buf[-200:]!r}")

    def run(self, cmd, prompt, timeout=30, want=None, bad=r"Unknown command|ERROR|Synchronous Abort|### ERROR"):
        self.sync(prompt)
        self.mark()
        self.send(cmd + "\r")
        self.expect("\n" + re.escape(prompt) + "$", timeout)
        body = self.buf.split("\n", 1)[1] if "\n" in self.buf else ""
        body = body[:len(body) - len(prompt)]
        if bad and re.search(bad, body):
            raise Fail(f"{cmd!r}: {body.strip()[-300:]!r}")
        if want and not re.search(want, body):
            raise Fail(f"{cmd!r}: wanted /{want}/, got {body.strip()[-300:]!r}")
        return body

    def checked(self, cmd, timeout):
        """Destructive command, installer prompt only: type it, require the echo to equal it,
        then Enter; any other echo gets Ctrl-C and nothing runs."""
        self.sync(INSTALLER_PROMPT)          # the output so far ends in "\n" + the installer prompt
        self.mark()
        self.send(cmd)
        end = time.monotonic() + 5
        while time.monotonic() < end and self.buf != cmd and cmd.startswith(self.buf):
            self.pump(0.05)
        if self.buf != cmd:                  # exactly the typed characters, nothing else
            self.send("\x03")
            raise Fail(f"echo {self.buf!r} != {cmd!r}: Ctrl-C sent, command NOT executed")
        self.send("\r")
        self.expect("\n" + re.escape(INSTALLER_PROMPT) + "$", timeout)
        body = self.buf[len(cmd):]
        if re.search(r"ERROR|refused|Unknown command|Usage", body) or not re.search(r": OK", body):
            raise Fail(f"{cmd!r}: {body.strip()[-300:]!r}. " + DONT_POWER_OFF)
        return body

    def crc(self, addr, n, prompt, timeout=120):
        out = self.run(f"crc32 {addr:#x} {n:#x}", prompt, timeout)
        m = re.search(r"==> ([0-9a-f]{8})", out)
        if not m:
            raise Fail(f"crc32 output {out!r}")
        return int(m.group(1), 16)

    def upload(self, addr, data, prompt):
        """loadx, or (sandbox sessions with fast=dir) the sandbox's hostfs: same bytes, same crc check."""
        if getattr(self, "fast", None):
            (self.fast / "upload.bin").write_bytes(data)
            self.run(f"load hostfs - {addr:#x} upload.bin", prompt, 30, want=rf"{len(data)} bytes read")
            if self.crc(addr, len(data), prompt) != zlib.crc32(data):
                raise Fail(f"upload {addr:#x}: RAM crc32 differs from the host's")
            return
        self.loadx(addr, data, prompt)

    def loadx(self, addr, data, prompt):
        self.sync(prompt)
        self.send(f"loadx {addr:#x}\r")
        ur.xmodem_send(self.t, data, wait=60, log=lambda m: self.f.write(f"\n[xmodem] {m}\n"))
        self.mark()
        self.expect("\n" + re.escape(prompt) + "$", 30)
        if self.crc(addr, len(data), prompt) != zlib.crc32(data):
            raise Fail(f"loadx {addr:#x}: RAM crc32 differs from the host's")


# ---------------------------------------------------------------- the write path
def read_compare(con, img, unknown, what):
    con.run("sf probe 0:0", INSTALLER_PROMPT, 30, want=r"mx25u25635f.*total 32 MiB")
    con.run(f"sf read {A_READ:#x} 0 {MODEL_LEN:#x}", INSTALLER_PROMPT, 300, want=": OK")
    for lo, hi in segments(unknown):
        got = con.crc(A_READ + lo, hi - lo, INSTALLER_PROMPT)
        if got != zlib.crc32(img[lo:hi]):
            raise Fail(f"flash {what}: {lo:#x}..{hi:#x} crc32 {got:08x} != model {zlib.crc32(img[lo:hi]):08x}. "
                       "STOP: the flash is not what the journal says")
    return f"0..{MODEL_LEN:#x} == model" + (f" (unknown {unknown} skipped)" if unknown else "")


def bundle(p):
    """One upload of every object in the plan, each at a 4 KiB boundary (r["ram"])."""
    b = bytearray()
    for r in p["regions"]:
        if not r["obj"]:
            continue
        b += b"\xff" * (-len(b) % SECTOR)
        r["ram"] = A_BUNDLE + len(b)
        b += r["obj"]
    if len(b) > A_READ - A_BUNDLE:
        raise Fail("upload bundle overlaps the read buffer")
    return bytes(b)


def write_region(con, a, cur, r, jpath):
    o, n, obj, s, want = r["off"], r["len"], r["obj"], r["split"], r["want"]
    cmds = [f"sf erase {o:#x} {n:#x}"]
    if obj:
        cmds += [f"sf write {r['ram'] + s:#x} {o + s:#x} {len(obj) - s:#x}", f"sf write {r['ram']:#x} {o:#x} {s:#x}"]
    if not a.write:
        say(f"dry run, would send ({r['name']}): " + "; ".join(cmds))
        return
    rid = uuid.uuid4().hex[:12]
    log({"kind": "write", "phase": "intent", "id": rid, "label": r["name"], "off": o, "len": n,
         "sha256": sha(want), "file": keep(want), "cmds": cmds}, jpath)
    if obj and con.crc(r["ram"], len(obj), INSTALLER_PROMPT) != zlib.crc32(obj):
        raise Fail(f"{r['name']}: RAM copy changed since the upload; nothing written")

    def readback(expect, what, lo=0, hi=0):
        con.run(f"sf read {A_RB:#x} {o:#x} {n:#x}", INSTALLER_PROMPT, 120, want=": OK")
        if con.crc(A_RB, n, INSTALLER_PROMPT) != zlib.crc32(expect):
            raise Fail(f"{r['name']}: read-back {what} differs. " + DONT_POWER_OFF)
        if hi > lo:                          # byte for byte against the uploaded object
            con.run(f"cmp.b {r['ram'] + lo:#x} {A_RB + lo:#x} {hi - lo:#x}", INSTALLER_PROMPT, 60,
                    want=r"were the same", bad=r"!=|differ")
    con.checked(cmds[0], 300)
    readback(b"\xff" * n, "after the erase")
    if obj:
        con.checked(cmds[1], 300)
        readback(b"\xff" * s + want[s:], "after the body", s, len(obj))
        con.checked(cmds[2], 60)
        readback(want, "after the header", 0, len(obj))
    log({"kind": "write", "phase": "done", "id": rid, "label": r["name"]}, jpath)
    cur[o:o + n] = want


def run_writes(con, a, img, unknown, p, jpath):
    """Upload, then each region in order. A failed BOOT_MODE/TOC2 step erases that sector again."""
    data = bundle(p)
    if data:
        con.upload(A_BUNDLE, data, INSTALLER_PROMPT)
        for r in p["regions"]:
            if r["obj"] and con.crc(r["ram"], len(r["obj"]), INSTALLER_PROMPT) != zlib.crc32(r["obj"]):
                raise Fail(f"{r['name']}: bundle part crc32 differs")
    cur = bytearray(img)
    con.run("sf probe 0:0", INSTALLER_PROMPT, 30, want=r"mx25u25635f.*total 32 MiB")   # a fresh handle
    for r in p["regions"]:
        try:
            write_region(con, a, cur, r, jpath)
        except Fail:
            if a.write and r["name"] in ("bootmode", "toc2"):
                say(f"{r['name']} failed: erasing {r['off']:#x} again")
                write_region(con, a, cur, region(r["name"] + "-recovery", r["off"], r["len"], b"", 0), jpath)
            raise
    return cur


# ---------------------------------------------------------------- hardware session
def ssh(a, cmd, timeout=60):
    return subprocess.run(shlex.split(a.ssh) + ["-o", "ConnectTimeout=10", "-o", "BatchMode=yes", HOST, cmd],
                          capture_output=True, text=True, errors="replace", timeout=timeout)


def lease(a, take=True):
    if take:
        r = ssh(a, f"mkdir /run/asic-lease 2>/dev/null && echo \"{a.owner} $(date +%s)\" > /run/asic-lease/owner "
                   f"&& echo GOT || cat /run/asic-lease/owner")
        if "GOT" not in r.stdout and not r.stdout.startswith(a.owner + " "):
            raise Fail(f"ASIC lease held by someone else: {r.stdout.strip()!r}")
    else:
        ssh(a, f"grep -q '^{a.owner} ' /run/asic-lease/owner && rm -rf /run/asic-lease")


def power(state):
    r = subprocess.run(["lab-power", PLUG, state], capture_output=True, text=True, timeout=60)
    if r.returncode:
        raise Fail(f"lab-power {PLUG} {state}: {r.stdout}{r.stderr}")


def attach(a):
    return Console(ur.Pipe(shlex.split(a.ssh) + ["-T", "-e", "none", "-o", "BatchMode=yes", CONSOLE, "attach", PORT],
                           escape=bytes([0x02, 0x1D]), take=True), a.transcript)


def mode_linux_check(a, img, unknown):
    r = subprocess.run(shlex.split(a.ssh) + ["-o", "ConnectTimeout=10", "-o", "BatchMode=yes", HOST,
                       "cat /dev/mtd0ro"], capture_output=True, timeout=120)
    d = r.stdout
    if r.returncode or len(d) != MODEL_LEN:
        raise Fail(f"/dev/mtd0ro read: rc {r.returncode}, {len(d)} B")
    bad = [hex(lo) for lo in range(0, MODEL_LEN, SECTOR)
           if not any(overlaps(lo, lo + SECTOR, *u) for u in unknown) and d[lo:lo + SECTOR] != img[lo:lo + SECTOR]]
    flags = ssh(a, "grep mtd0 /proc/mtd; cat /sys/class/mtd/mtd0/flags").stdout.strip().replace("\n", " | ")
    if bad:
        raise Fail(f"/dev/mtd0ro differs from the model in {len(bad)} sector(s): {bad[:8]}")
    return {"mtd0ro": f"== model (sha256 {sha(d)[:16]})", "mtd0": flags}


def to_vendor_prompt(con, a, img):
    """Reboot into the vendor prompt. With TOC2 + BOOT_MODE live, stage3 boots our app unless 'u'
    reaches its poll: keys every 20 ms from stage2's banner (PROGRESS 2026-11-15)."""
    ours = a3.toc_at(img, TOC2)[1] is not None and boot_mode_app(img) != 5
    con.mark()
    if ours or a.power_cycle:
        power("off")
        time.sleep(12)
        power("on")
    else:
        ssh(a, "systemctl reboot", 30)
    if ours:
        con.expect(r"stage2_loader", 120)
        end = time.monotonic() + 30
        while time.monotonic() < end and not re.search(r"Executing application|User request for loading", con.buf):
            con.send("u")
            con.pump(0.02)
        con.expect(r"User request for loading U-Boot", 10)
    con.expect(r"Type 123<ENTER> to STOP autoboot", 400)
    con.send("123\r")
    con.expect("\n" + re.escape(VENDOR_PROMPT) + "$", 20)
    con.run("version", VENDOR_PROMPT, 10, want=r"2018\.05-alpine_db")


def start_installer(con, a):
    ubin = pathlib.Path(a.installer) / "u-boot.bin"
    data = ubin.read_bytes()
    if b"2026.10-cs8210-installer" not in data:
        raise Fail(f"{ubin}: not the #uboot-cs8210-installer build")
    con.loadx(LOAD, data, VENDOR_PROMPT)
    con.sync(VENDOR_PROMPT)
    con.mark()
    con.send(f"booti {LOAD:#x} - $" "{fdtaddr}\r")
    con.expect("\n" + re.escape(INSTALLER_PROMPT) + "$", 90)
    if "Entry: EL2" not in con.buf:
        raise Fail("installer banner without 'Entry: EL2': stop, power-cycle (nothing written)")
    con.run("version", INSTALLER_PROMPT, 10, want=r"2026\.10-cs8210-installer")


def wait_linux(a, timeout=600):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            r = ssh(a, "systemctl is-system-running; journalctl -b --no-pager | grep -m1 'cage guard'", 30)
            if "cage guard" in r.stdout:
                if "within allowed cages" not in r.stdout:
                    raise Fail("cage guard refused: other cages populated (house-network rule)")
                return r.stdout.strip().replace("\n", " | ")
        except subprocess.TimeoutExpired:
            pass
        time.sleep(10)
    raise Fail("NixOS did not come back")


def mode_hw(a, img, unknown):
    st = {}
    p = plan(a.mode, img, a.payload, a.slot)
    st["plan"] = plan_summary(p)
    stray = [u for u in unknown if not any(r["off"] <= u[0] and u[1] <= r["off"] + r["len"] for r in p["regions"])]
    if stray:
        raise Fail(f"unknown flash ranges {stray} outside this run's regions: status/rollback first")
    blockers = []
    if a.mode != "rollback":
        g = json.loads(GATES.read_text()) if GATES.exists() else {}
        over = dict(o.split("=", 1) for o in a.override_gate)
        blockers = [f"gate {n} open" for n in GATE_NAMES if not g.get(n) and n not in over]
        recs = journal()
        for kind in ("verify", "rehearse"):
            if not any(r.get("kind") == kind and r.get("plan_sha") == p["plan_sha"] and r.get("pass") for r in recs):
                blockers.append(f"no passing {kind} of plan {p['plan_sha'][:16]}")
        st["gate_overrides"] = over
    if blockers and a.write:
        raise Fail("; ".join(blockers) + " (spi-upgrade-plan.md; --override-gate NAME=REASON)")
    st["would_refuse"] = blockers
    lease(a)
    st["linux"] = mode_linux_check(a, img, unknown) if not unknown else "skipped (unknown ranges)"
    con = attach(a)
    try:
        ur.settle(con.t, 0.8, 10)
        to_vendor_prompt(con, a, img)
        start_installer(con, a)
        st["before"] = read_compare(con, img, unknown, "before")
        new = run_writes(con, a, img, unknown, p, JOURNAL)
        if a.write:
            st["after"] = read_compare(con, new, [], "after")
        else:
            st["dry_run"] = "every read-only step passed; writes printed only"
    finally:
        con.mark()
        power("off")
        time.sleep(12)
        power("on")
        st["power_cycled"] = True
    try:
        if a.write and a.mode != "rollback":
            con.expect(r"Loading application to 08000000", 180, fail=r"Invalid application size|load failed")
            con.expect(a.expect_banner, 60)
            st["cold_boot"] = "stage3 loaded our app from SPI; its banner came up"
        elif a.write:
            con.expect(r"Loading application to 01100000", 180, fail=r"Loading application to 08000000")
            st["cold_boot"] = "stage3 loaded the factory U-Boot"
    finally:
        con.t.close()
    if not a.write or a.mode == "rollback" or a.boots_linux:
        st["linux_after"] = wait_linux(a)
        lease(a)
        if a.write:
            st["linux_check_after"] = mode_linux_check(a, *model())
        lease(a, take=False)
    return st


# ---------------------------------------------------------------- sandbox rehearsal
def mode_rehearse(a, img, unknown):
    import tempfile
    import bootchain_emu as be
    if not a.uboot:
        raise Fail("--uboot (#uboot-sandbox-cs8210-installer) is required")
    p = plan("install", img, a.payload, a.slot)
    payload_sha = sha(p["regions"][0]["obj"][a3.HDR:-4])
    sb, res, cuts = pathlib.Path(a.uboot), {}, []
    WRITES.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=str(WRITES)) as td:
        td = pathlib.Path(td)

        def session(fast=True):
            con = Console(PtyProc([str(sb / "u-boot"), "-t", "raw", "-d", str(sb / "u-boot.dtb")], td),
                          a.transcript)
            con.fast = td if fast else None       # XMODEM in the full install run, hostfs elsewhere
            con.expect("\n" + re.escape(INSTALLER_PROMPT) + "$", 30)
            con.run("version", INSTALLER_PROMPT, 10, want=r"U-Boot 2026\.10")
            return con
        # 1. dry run, then the real install, on a file copy of the 32 MiB flash
        (td / "spi.bin").write_bytes(full_image(img))
        jpath = td / "journal.jsonl"
        for write in (False, True):
            a.write = write
            con = session(fast=not write)
            try:
                res[f"read before ({'write' if write else 'dry run'})"] = read_compare(con, img, [], "before")
                new = run_writes(con, a, img, [], p, jpath)
                if write:
                    res["read after"] = read_compare(con, new, [], "after")
            finally:
                con.t.close()
            if not write and (td / "spi.bin").read_bytes() != full_image(img):
                raise Fail("the dry run changed the sandbox flash")
        flash = (td / "spi.bin").read_bytes()
        if flash[:MODEL_LEN] != bytes(p["after"]) or flash[MODEL_LEN:] != bytes(full_image(img)[MODEL_LEN:]):
            raise Fail("sandbox flash after the install != the plan's image")
        m_after, unk = model(jpath)
        if bytes(m_after) != bytes(p["after"]) or unk:
            raise Fail("journal model after the install != the plan")
        got = classify(be.boot(flash), payload_sha)
        res["emulated stage2/3 on the sandbox's flash"] = got
        if got != "ours:" + p["slot"]:
            raise Fail(f"sandbox result boots {got}")
        # 2. refusals: outside the windows, update, protect: the flash file must not change
        con = session()
        bad = ("sf erase 0x40000 0x1000", "sf erase 0x1f000 0x2000", "sf erase 0x0 0x2000000",
               f"sf write {A_BUNDLE:#x} 0x190000 0x100", f"sf write {A_BUNDLE:#x} 0x2f8f00 0x200",
               "sf erase 0x380000 0x1000", f"sf update {A_BUNDLE:#x} 0x20000 0x100", "sf protect lock 0 0x1000",
               f"sf write {A_BUNDLE:#x} 0x1fff00 0x200")
        try:
            con.run("sf probe 0:0", INSTALLER_PROMPT, 30, want=r"mx25u25635f")
            for c in bad:
                con.sync(INSTALLER_PROMPT)
                con.mark()
                con.send(c + "\r")
                con.expect("\n" + re.escape(INSTALLER_PROMPT) + "$", 30)
                if not re.search(r"outside the write window, refused|Usage:|usage", con.buf):
                    raise Fail(f"sandbox accepted {c!r}: {con.buf!r}")
        finally:
            con.t.close()
        if (td / "spi.bin").read_bytes() != flash:
            raise Fail("a refused command changed the sandbox flash")
        res["refusals"] = f"{len(bad)}/{len(bad)} refused, flash file unchanged"
        # 3. power cuts: kill the sandbox right after each destructive command
        a.write = True
        ncmd = sum(3 if r["obj"] else 1 for r in p["regions"])
        for k in range(1, ncmd + 1):
            (td / "spi.bin").write_bytes(full_image(img))
            jk = td / f"journal-cut{k}.jsonl"
            con = session()
            sent, orig = [0], con.checked

            def counting(cmd, timeout, orig=orig, k=k, con=con, sent=sent):
                out = orig(cmd, timeout)
                sent[0] += 1
                if sent[0] == k:
                    con.t.kill()
                    raise KeyboardInterrupt
                return out
            con.checked = counting
            try:
                run_writes(con, a, img, [], p, jk)
            except KeyboardInterrupt:
                pass
            finally:
                con.t.close()
            st = (td / "spi.bin").read_bytes()
            got = classify(be.boot(st), payload_sha)
            gu = classify(be.boot(st, b"u" * 8), payload_sha)
            _, unk = model(jk)
            cuts.append(f"after destructive command {k}/{ncmd}: boots {got}, with u {gu}; "
                        f"journal unknown {[(hex(x), hex(y)) for x, y in unk]}")
            if got not in ("factory", "ours:" + p["slot"]) or gu != "factory":
                raise Fail(cuts[-1])
    res["power cuts"] = cuts
    log({"kind": "rehearse", "plan_sha": p["plan_sha"], "pass": True, "sandbox": str(sb), "cuts": len(cuts)})
    return res


# ---------------------------------------------------------------- main
def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("mode", choices=["build", "verify", "rehearse", "status", "linux-check",
                                     "install", "update", "switch", "rollback"])
    ap.add_argument("--payload", help="our standalone U-Boot u-boot.bin (stage3 app, load 0x08000000)")
    ap.add_argument("--slot", type=int, choices=(0, 1), help="install: slot (default A); update/switch: target")
    ap.add_argument("--installer", help="#uboot-cs8210-installer output directory")
    ap.add_argument("--uboot", help="rehearse: #uboot-sandbox-cs8210-installer output directory")
    ap.add_argument("--write", action="store_true", help="really write SPI (default: dry run)")
    ap.add_argument("--override-gate", action="append", default=[], metavar="NAME=REASON")
    ap.add_argument("--ssh", default="ssh", help="ssh command (e.g. 'sudo -n ssh')")
    ap.add_argument("--owner", default="spi-upgrade", help="ASIC lease owner tag")
    ap.add_argument("--power-cycle", action="store_true", help="reach the vendor prompt by power cycle, not ssh reboot")
    ap.add_argument("--expect-banner", default=r"U-Boot 2026\.10", help="cold boot: our banner regex")
    ap.add_argument("--boots-linux", action="store_true", help="after a write, wait for NixOS + check mtd0ro")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--out", help="verify/rehearse: also write the result to this file")
    ap.add_argument("--transcript", default=str(DEV / time.strftime("serial-spiupgrade-%Y%m%d-%H%M%S.txt")))
    a = ap.parse_args()
    for o in a.override_gate:
        if "=" not in o or o.split("=")[0] not in GATE_NAMES:
            ap.error(f"--override-gate NAME=REASON, NAME in {GATE_NAMES}")
    st = {"mode": a.mode, "write": a.write}
    hw = a.mode in ("install", "update", "switch", "rollback")
    try:
        img, unknown = model()
        if a.mode == "build":
            p = plan("install" if a3.toc_at(img, TOC2)[1] is None else "update", img, a.payload, a.slot)
            st.update(plan_summary(p), dir=str(write_build(p)))
        elif a.mode == "verify":
            st.update(plan_summary(mode_verify(a, img, unknown)))
        elif a.mode == "rehearse":
            st.update(mode_rehearse(a, img, unknown))
            if a.out:
                pathlib.Path(a.out).write_text(json.dumps(st, indent=1) + "\n")
        elif a.mode == "status":
            st["toc2"] = "valid (shadows 0x40000)" if a3.toc_at(img, TOC2)[1] else "absent: factory TOC rules"
            bm = boot_mode_app(img)
            st["boot_mode_app"] = hex(bm) if bm is not None else "VALID HEADER, BAD DATA: hangs; erase 0x30000"
            for n, (o, mx) in SLOTS.items():
                s, f = a3.obj_check(img, o, mx)
                st[f"slot{'AB'[n]}"] = s + (f", {f['size']:#x} B" if f else "")
            st["unknown"] = unknown
        elif a.mode == "linux-check":
            lease(a)
            try:
                st.update(mode_linux_check(a, img, unknown))
            finally:
                lease(a, take=False)
        else:
            if not a.installer:
                ap.error("--installer (#uboot-cs8210-installer) is required")
            st.update(mode_hw(a, img, unknown))
    except (Fail, subprocess.SubprocessError, OSError) as e:
        st["failed"] = f"{type(e).__name__}: {e}"
    except SystemExit as e:              # uart_recovery's xmodem raises SystemExit(str)
        if not isinstance(e.code, str):
            raise
        st["failed"] = e.code
    if hw:
        log({"kind": a.mode, **{k: v for k, v in st.items() if k != "mode"}})
    print(json.dumps(st, indent=1, default=str))
    return 1 if "failed" in st else 0


if __name__ == "__main__":
    sys.exit(main())
