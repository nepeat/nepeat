#!/usr/bin/env python3
"""Put our U-Boot in a DMC-12's boot SPI flash (IPXE slot + BOOT_MODE), keeping the factory U-Boot
as the `u`-key fallback. Spec, gates and failure handling: spi-upgrade-plan.md.

Modes (all through the logged tmux pane `serial-dmc12`, like reimage.py):
  status    read-only: factory prompt, identity, OTP, fresh CS0 read == host model
  rehearse  zero SPI writes: Route A boots a RAM image holding the exact bytes `install`/`update`
            would write, then the same image with `u` shows the escape. Recorded per payload.
  install   first install: IPXE 0 + BOOT_MODE (app 0x110)
  update    write the inactive IPXE instance, validate, then flip BOOT_MODE
  rollback  erase BOOT_MODE: stage3 boots the untouched factory U-Boot again
  switch    --to ipxe0|ipxe1|factory: rewrite only BOOT_MODE (the target slot must hold a valid
            object that already booted on this unit, or was rehearsed with that BOOT_MODE)
  slots     what is in UBOOT/IPXE/BOOT_MODE per the verified model (--live: also power-cycle,
            stop our U-Boot and prove by CRC which slot is running)
  clipcheck offline: compare external-programmer dumps of CS0 (--dump, twice) with the model

Nothing is written without --write: every read-only step runs and the write commands are printed.
With --write each write asks for the unit id (--yes: no questions, for agents).

  spi-upgrade.py --unit u01 --plug 2 status
  spi-upgrade.py --unit u01 --plug 2 rehearse --ip 10.60.2.10
  spi-upgrade.py --unit u01 --plug 2 install --ip 10.60.2.10           # dry run
  spi-upgrade.py --unit u01 --plug 2 install --ip 10.60.2.10 --write
  spi-upgrade.py --unit u01 clipcheck --dump cs0-a.bin --dump cs0-b.bin

Host model of CS0 = dumps/<unit>/mtd0.bin (verified against its manifest) with every verified
region write in inventory/<unit>/spi-upgrade.jsonl replayed. A region is journaled as `intent`
(with its expected content in dumps/<unit>/spi-writes/) before the first command and `done` after
its read-back matched; an intent without done is an *unknown* range, left out of the comparisons
(rollback still works) and allowed in install/update only inside the region being rewritten.
Destructive commands are typed, their echo checked character for character, and only then sent.
Objects are written data first, header last: a torn write leaves a bad header (stage3: app 5),
never a good header with bad data (stage3 hangs past its key poll, so no `u`; uart-boot.md).
Only 0..0xfd0000 is modelled (the env pair above changes with every saveenv).
Never `saveenv`/`env default`/`delenv` here.
"""
import argparse, hashlib, json, pathlib, re, struct, subprocess, sys, time, uuid, zlib

ROOT = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
import alboot     # noqa: E402
import reimage    # noqa: E402  (console, pane, manifest + identity helpers)
import uartboot   # noqa: E402  (TOC slots, object wrapper, Route A)

Fail = reimage.Fail
SESSION = reimage.SESSION
PROMPT = "ALPINE_DB>"
MODEL_END = 0xFD0000                     # env pair 0xfd0000..0xffffff is not modelled
BOOT_MODE_OFF, SECTOR = 0x340000, 0x1000
IPXE_SLOTS = {0: (0x2C0000, 0x80000), 1: (0x580000, 0x80000)}
WRITABLE = {*IPXE_SLOTS.values(), (BOOT_MODE_OFF, SECTOR)}   # the only (off, len) ever erased
IPXE, BOOT_MODE, UBOOT_ID = 0x110, 0, 5
BM_DATA = 0x40                           # stage3 loads at most 0x40 B of BOOT_MODE data
PAYLOAD_LOAD = 0x08000000                # IPXE object: load = exec (stage3 jumps there at EL2)
# Factory U-Boot RAM staging (below its relocated self, above the 0x01xxxxxx stage3 area).
A_PAYLOAD, A_BOOTMODE, A_READBACK, A_CS0 = 0x09000000, 0x09F00000, 0x0C000000, 0x0A000000
OURS = r"U-Boot 2026\.10-dmc12-selfinit"
FACTORY = r"U-Boot 2015\.07"
USER_U = r"User request for loading U-Boot"
BAD = r"Kernel panic|Synchronous Abort|### ERROR|Unknown command|Wrong Image"
GATES = ROOT / "spi-upgrade-gates.json"
GATE_NAMES = ("clip_read", "review", "gpio22", "cold_pcie")
WRITE_MODES = ("install", "update", "rollback", "switch")
DONT_POWER_OFF = ("do NOT power off: the factory U-Boot is still running and can restore the region "
                  "from the backup (spi-recovery.md, the `sf update` recipe)")


def say(a, msg):
    if not a.json:
        print(f"[spi-upgrade] {msg}", flush=True)


def sha(b):
    return hashlib.sha256(b).hexdigest()


def journal_path(unit):
    return ROOT / "inventory" / unit / "spi-upgrade.jsonl"


def journal(unit):
    p = journal_path(unit)
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()] if p.exists() else []


def log(unit, rec):
    p = journal_path(unit)
    p.parent.mkdir(parents=True, exist_ok=True)
    rec = {"t": time.strftime("%Y-%m-%dT%H:%M:%S%z"), **rec}
    with p.open("a") as f:
        f.write(json.dumps(rec) + "\n")
    return rec


# ---- host model ---------------------------------------------------------------------------
def model(unit):
    """(expected CS0 0..MODEL_END, unknown ranges): the verified backup plus every verified write."""
    have, missing = reimage.manifest_ok(unit)
    if "mtd0.bin" in missing:
        raise Fail(f"no verified mtd0.bin for {unit} in dumps/{unit}/manifest.jsonl (acquire.py first)")
    raw = alboot.load(str(ROOT / "dumps" / unit / "mtd0.bin.zst"))
    if sha(raw) != have["mtd0.bin"]["sha256"]:
        raise Fail(f"dumps/{unit}/mtd0.bin.zst does not match its manifest sha256")
    img, unknown = bytearray(raw[:MODEL_END]), []
    recs = journal(unit)
    done = {r["id"] for r in recs if r.get("kind") == "write" and r.get("phase") == "done"}
    for r in recs:
        if r.get("kind") != "write" or r.get("phase") != "intent":
            continue
        off, n = r["off"], r["len"]
        if r["id"] not in done:
            unknown.append((off, off + n))
            continue
        data = (ROOT / r["file"]).read_bytes()
        if len(data) != n or sha(data) != r["sha256"]:
            raise Fail(f"{r['file']} does not match its journal record")
        img[off:off + n] = data
        unknown = [u for u in unknown if not (off <= u[0] and u[1] <= off + n)]
    return img, unknown


def boot_mode_app(img):
    """App stage3 will load (5 = factory U-Boot: BOOT_MODE blank, bad header or data > 0x40 B).
    A good header with bad data hangs stage3 after its key poll: refuse to call that bootable."""
    off, mx = uartboot.slot(img, BOOT_MODE, 0)
    st, f = alboot.check_object(img, off, mx, BOOT_MODE)
    if st == "OK" and f["size"] <= BM_DATA:
        return struct.unpack_from("<I", img, off + alboot.HDR + 4)[0]
    if f is None or f["size"] > BM_DATA:
        return UBOOT_ID
    raise Fail(f"BOOT_MODE has a valid header but {st}: stage3 hangs before the `u` poll. Erase "
               f"{BOOT_MODE_OFF:#x} from a running factory U-Boot, or use the clip")


def boot_mode_obj(app):
    # 0x40 B of data like the factory object (0x8c B wrapped); stage3 reads word +4 only.
    return uartboot.obj(BOOT_MODE, 0, struct.pack("<II", 0, app).ljust(BM_DATA, b"\0"))


def check_layout(img):
    """No unknown (e.g. SIG/PUK) objects; BOOT_MODE and both IPXE slots exactly where we expect."""
    unknown = [hex(i) for i, n, off, mx in uartboot.toc_entries(img) if i not in alboot.NAMES]
    if unknown:
        raise Fail(f"TOC has unknown object ids {unknown}: not a layout this script knows")
    off, mx = uartboot.slot(img, BOOT_MODE, 0)    # 64 KiB slot; only its first sector is used
    if off != BOOT_MODE_OFF or mx < SECTOR:
        raise Fail(f"BOOT_MODE slot is {off:#x}/{mx:#x}, expected 0x340000 and >= 4 KiB")
    for n, want in IPXE_SLOTS.items():
        if uartboot.slot(img, IPXE, n) != want:
            raise Fail(f"IPXE {n} slot is {uartboot.slot(img, IPXE, n)}, expected {want}")


def payload_ok(a):
    data = a.payload.read_bytes()
    st, f = alboot.check_object(data, 0, len(data), IPXE)
    if st != "OK":
        raise Fail(f"{a.payload}: {st} (expect a wrapped IPXE object, alboot.py check)")
    if f.get("load") != PAYLOAD_LOAD or f.get("exec") != PAYLOAD_LOAD:
        raise Fail(f"{a.payload}: load/exec {f.get('load'):#x}/{f.get('exec'):#x}, need {PAYLOAD_LOAD:#x}")
    if not re.search(OURS.replace("\\", "").encode(), data):
        raise Fail(f"{a.payload}: no '{OURS}' banner string (wrong build?)")
    return data


def plan(a, img):
    """(inst, app, payload, bootmode) for install/update/rehearse."""
    data = payload_ok(a)
    cur = boot_mode_app(img)
    if a.mode == "install" or (a.mode == "rehearse" and cur & 0x0FFFFFFF != IPXE):
        if cur != UBOOT_ID:
            raise Fail(f"BOOT_MODE already selects app {cur:#x}: use update or rollback")
        inst = 0
    else:
        if cur & 0x0FFFFFFF != IPXE:
            raise Fail(f"BOOT_MODE selects app {cur:#x}, not IPXE: use install")
        inst = 1 - (cur >> 28)
    off, mx = IPXE_SLOTS[inst]
    if len(data) > mx:
        raise Fail(f"payload {len(data):#x} B does not fit IPXE {inst} ({mx:#x} B)")
    app = IPXE | inst << 28
    return inst, app, data, boot_mode_obj(app)


def region(img, off, length, obj):
    """Expected content of [off, off+length) after erasing it and writing `obj` at its start."""
    return obj + b"\xff" * (length - len(obj))


def overlaps(r, lo, hi):
    return r[0] < hi and lo < r[1]


# ---- gates --------------------------------------------------------------------------------
def gates(a):
    g = json.loads(GATES.read_text()) if GATES.exists() else {}
    over = dict(o.split("=", 1) for o in a.override_gate)
    return [n for n in GATE_NAMES if not g.get(n) and n not in over], over


def rehearsed(unit, data, bm):
    for r in journal(unit):
        if (r.get("kind") == "rehearsal" and r.get("pass") and r.get("escape_pass")
                and r.get("payload_sha256") == sha(data) and r.get("bootmode_sha256") == sha(bm)):
            return r
    return None


# ---- console --------------------------------------------------------------------------------
def con(cmd, timeout=30):
    return reimage.console(cmd, timeout=timeout)


def tmux(*args):
    subprocess.run(["tmux", *args], check=True, capture_output=True)


def con_checked(cmd, timeout):
    """Destructive command: type it, require the echoed line to equal it exactly, then Enter.
    A garbled echo gets Ctrl-C and nothing is executed."""
    tmux("clear-history", "-t", SESSION)
    if not reimage.pane().rstrip().endswith(PROMPT):
        raise Fail(f"not at an idle {PROMPT} prompt; {cmd!r} not sent")
    for byte in cmd.encode():
        tmux("send-keys", "-t", SESSION, "-H", f"{byte:02x}")
        time.sleep(0.02)
    want = f"{PROMPT} {cmd}"
    deadline = time.monotonic() + 5
    while True:
        lines = reimage.pane().rstrip("\n").splitlines()
        last = lines[-1].rstrip() if lines else ""
        if last == want:
            break
        if time.monotonic() > deadline:
            tmux("send-keys", "-t", SESSION, "C-c")
            raise Fail(f"echo {last!r} != {want!r}: Ctrl-C sent, command NOT executed")
        time.sleep(0.1)
    base = len(lines)
    tmux("send-keys", "-t", SESSION, "Enter")
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        new = reimage.pane().rstrip("\n").splitlines()[base:]
        if new and new[-1].rstrip() == PROMPT:
            return "\n".join(new[:-1])
        time.sleep(0.2)
    raise Fail(f"{cmd!r}: no prompt after {timeout}s. " + DONT_POWER_OFF)


def crc(addr, size, timeout=120):
    return uartboot.crc_of(con(f"crc32 {addr:#x} {size:#x}", timeout))


def run(cmd, timeout):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def power(a, state):
    if state == "off" and a.ip:          # a running NixOS: flush the SSD first (best effort)
        try:
            run(reimage.SSH + [f"root@{a.ip}", "sync"], 15)
        except subprocess.TimeoutExpired:
            pass
    r = run([sys.executable, str(ROOT.parents[1] / "tools" / "lab_power.py"), str(a.plug), state], 30)
    if r.returncode:
        raise Fail(f"lab_power {a.plug} {state} failed: {r.stdout[-200:]}{r.stderr[-200:]}")


def counts(pats):
    t = reimage.pane()
    return {p: len(re.findall(p, t)) for p in pats}


def stage3_u(a):
    """Power-cycle sending `u` to stage3 (stage3keys.py timing), stop the autoboot that follows.
    Returns which of: user request / factory banner / our banner appeared (new matches only)."""
    pats = {"user": USER_U, "factory": FACTORY, "ours": OURS, "exec": "Executing next",
            "loading": "Loading application", "hit": "Hit any key"}
    power(a, "off")
    time.sleep(a.off)
    tmux("clear-history", "-t", SESSION)
    base = counts(pats.values())
    power(a, "on")
    t0, seen = time.monotonic(), set()
    while time.monotonic() - t0 < 120:
        c = counts(pats.values())
        seen = {k for k, p in pats.items() if c[p] > base[p]}
        if "exec" in seen and "loading" not in seen:
            tmux("send-keys", "-t", SESSION, "-l", "u")
        if "hit" in seen:
            tmux("send-keys", "-t", SESSION, "-l", " ")
            break
        time.sleep(0.02)
    deadline = time.monotonic() + 20
    while not reimage.pane().rstrip().endswith(PROMPT) and time.monotonic() < deadline:
        time.sleep(0.2)
    c = counts(pats.values())
    seen = {k for k, p in pats.items() if c[p] > base[p]}
    seen.add("prompt") if reimage.pane().rstrip().endswith(PROMPT) else None
    return seen


def to_factory(a, img, unknown):
    """Stop at the factory prompt. With our U-Boot selected, the `u` key is the only way there."""
    bm_unknown = any(overlaps(u, BOOT_MODE_OFF, BOOT_MODE_OFF + SECTOR) for u in unknown)
    if not bm_unknown and boot_mode_app(img) == UBOOT_ID:
        r = run([sys.executable, str(ROOT / "powercycle_to_uboot.py"), "--plug", str(a.plug)], 240)
        if r.returncode or PROMPT not in r.stdout:
            raise Fail(f"did not reach the factory prompt: {r.stdout[-300:]}{r.stderr[-300:]}")
        return
    seen = stage3_u(a)       # `u` is silent when BOOT_MODE is blank: the banner + prompt decide
    if not {"factory", "prompt"} <= seen or "ours" in seen:
        raise Fail(f"`u` did not reach the factory prompt: {sorted(seen)}")


def identify(a, st):
    out = con("printenv bootp_dci board_name")
    vals = dict(l.split("=", 1) for l in out.splitlines() if "=" in l)
    want = reimage.expected_serial(a.unit)
    if want is None or vals.get("bootp_dci") != want:
        raise Fail(f"board serial does not match {a.unit} in units-private.txt; wrong box?")
    st["board_name"] = vals.get("board_name")
    inv = ROOT / "inventory" / a.unit / "uboot-printenv-before-spi-upgrade-private.txt"
    if not inv.exists():
        text = con("printenv")
        if "bootcmd=" not in text:
            raise Fail("printenv has no bootcmd; console out of sync?")
        inv.write_text(text + "\n")
        inv.chmod(0o600)


def state_checks(a, st):
    if "- Secure boot: disabled" not in con("otp show direct"):
        raise Fail("OTP does not say 'Secure boot: disabled': stop (spi-uboot-replacement.md §6)")
    st["otp"] = "secure boot disabled"


def segments(unknown):
    """[lo, hi) pieces of 0..MODEL_END outside the unknown ranges."""
    out, lo = [], 0
    for u0, u1 in sorted(unknown):
        if u0 > lo:
            out.append((lo, u0))
        lo = max(lo, u1)
    return out + ([(lo, MODEL_END)] if lo < MODEL_END else [])


def read_cs0(a, img, unknown, what):
    """sf probe + full read of 0..MODEL_END; crc32 of every known piece must equal the model."""
    if "mx25u12835f" not in con("sf probe 0:0").lower():
        raise Fail("sf probe 0:0 did not detect the MX25U12835F")
    con(f"sf read {A_CS0:#x} 0 {MODEL_END:#x}", a.read_timeout)
    for lo, hi in segments(unknown):
        got, want = crc(A_CS0 + lo, hi - lo, 300), zlib.crc32(img[lo:hi])
        if got != want:
            raise Fail(f"CS0 {what} {lo:#x}..{hi:#x}: crc32 {got:08x} != model {want:08x}. STOP: the "
                       "flash is not what the journal says (spi-upgrade-plan.md, failure handling)")
    return f"{zlib.crc32(img):08x}" if not unknown else f"known pieces match, unknown {unknown}"


# ---- steps ----------------------------------------------------------------------------------
def stage(a, files):
    d = a.tftp_root / a.tftp_dir
    d.mkdir(parents=True, exist_ok=True)
    for name, data in files.items():
        (d / name).write_bytes(data)
    con("setenv autoload no")
    if "bound to address" not in con("dhcp", 60):
        raise Fail("dhcp failed")
    con(f"setenv serverip {a.server}")          # after dhcp: it overwrites serverip


def tftp(a, addr, name, data):
    con(f"tftpboot {addr:#x} {a.tftp_dir}/{name}", 180)
    if crc(addr, len(data)) != zlib.crc32(data):
        raise Fail(f"{name}: RAM crc32 does not match the host")


def confirm(a, what):
    if a.yes:
        say(a, f"--yes: {what}")
        return
    if not sys.stdin.isatty():
        raise Fail(f"needs confirmation ({what}); rerun with --yes")
    if input(f"{what}. Type the unit id ({a.unit}) to continue: ").strip() != a.unit:
        raise Fail("not confirmed")


def keep(a, data):
    d = ROOT / "dumps" / a.unit / "spi-writes"
    d.mkdir(parents=True, exist_ok=True)
    p = d / f"{sha(data)[:16]}.bin"
    p.write_bytes(data)
    return str(p.relative_to(ROOT))


def write_region(a, img, off, length, obj, ram, label):
    """Erase [off, off+length), write `obj` (in RAM at `ram`) data first, header last; read back.
    Returns the new model. Dry run: prints the commands and returns the expected model."""
    if (off, length) not in WRITABLE:
        raise Fail(f"refusing to write {off:#x}/{length:#x}: not one of {sorted(WRITABLE)}")
    want = region(img, off, length, obj)
    cmds = [f"sf erase {off:#x} {length:#x}"]
    if obj:
        h = alboot.HDR
        cmds += [f"sf write {ram + h:#x} {off + h:#x} {len(obj) - h:#x}", f"sf write {ram:#x} {off:#x} {h:#x}"]
    new = bytearray(img)
    new[off:off + length] = want
    if not a.write:
        say(a, f"dry run, would send ({label}): " + "; ".join(cmds))
        return new
    confirm(a, f"[WRITES SPI] {label}: {'; '.join(cmds)}")
    rid = uuid.uuid4().hex[:12]
    log(a.unit, {"kind": "write", "phase": "intent", "id": rid, "label": label, "off": off,
                 "len": length, "sha256": sha(want), "file": keep(a, want), "cmds": cmds})
    if "mx25u12835f" not in con("sf probe 0:0").lower():   # fresh handle (the validator re-probes)
        raise Fail("sf probe 0:0 did not detect the MX25U12835F")
    if obj and crc(ram, len(obj)) != zlib.crc32(obj):
        raise Fail(f"{label}: RAM copy changed since TFTP; nothing written")
    for c in cmds:
        out = con_checked(c, 180)
        if "OK" not in out or re.search(r"ERROR|FAIL", out, re.I):
            raise Fail(f"{c!r}: {out.strip()[-200:]!r}. " + DONT_POWER_OFF)
    con(f"sf read {A_READBACK:#x} {off:#x} {length:#x}", 120)
    if crc(A_READBACK, length) != zlib.crc32(want):
        raise Fail(f"{label}: read-back crc32 differs from what was written. " + DONT_POWER_OFF)
    log(a.unit, {"kind": "write", "phase": "done", "id": rid, "label": label})
    return new


def validate(a, oid, inst, label, expect=True):
    out = con(f"flash_contents_obj_validate {alboot.NAMES[oid]} {inst} && echo VALID-OK || echo VALID-FAIL")
    ok = "VALID-OK" in out and "failed" not in out
    bad = "VALID-FAIL" in out
    if (expect and not ok) or (not expect and not bad):
        raise Fail(f"{label}: factory flash_contents_obj_validate gave {out.strip()[-200:]!r}, "
                   f"expected {'OK' if expect else 'FAIL'}")


def bootmode_clear(a, img, why):
    """Erase the BOOT_MODE sector again (stage3 -> factory U-Boot) after a failed BOOT_MODE step."""
    say(a, f"{why}: erasing BOOT_MODE again so stage3 boots the factory U-Boot")
    return write_region(a, img, BOOT_MODE_OFF, SECTOR, b"", 0, "erase BOOT_MODE (recovery)")


def since(marker):
    """Pane text after the last `marker` line (console.py's clear-history calls make match
    counts against an earlier baseline unreliable; a marker typed just before is not)."""
    t = reimage.pane()
    i = t.rfind(marker)
    return t[i:] if i >= 0 else ""


def wait_boot(a, base, timeout, want_ours=True, after=None):
    """After a power-on: which U-Boot came up, then the NixOS login prompt."""
    pats = {"ours": OURS, "factory": FACTORY, "x0": r"x0=0x0\b", "login": r"root@dmc12[^\n]*#", "bad": BAD}
    t0, seen = time.monotonic(), {}
    if after:
        base = {}
    while time.monotonic() - t0 < timeout:
        t = since(after) if after else reimage.pane()
        for k, p in pats.items():
            if k not in seen and len(re.findall(p, t)) > base.get(p, 0):
                seen[k] = round(time.monotonic() - t0, 1)
        if "bad" in seen or "login" in seen:
            break
        time.sleep(0.5)
    if "bad" in seen and (want_ours or "login" not in seen):
        raise Fail(f"bad console output during boot: {seen}")
    if want_ours and ("ours" not in seen or "factory" in seen or "x0" not in seen):
        raise Fail(f"expected our U-Boot straight from stage3 (banner + x0=0x0, no factory U-Boot): {seen}")
    if not want_ours and "factory" not in seen:
        raise Fail(f"expected the factory U-Boot: {seen}")
    if "login" not in seen:
        raise Fail(f"no NixOS login within {timeout}s: {seen}")
    return seen


def power_cycle(a, want_ours, timeout=420):
    power(a, "off")
    time.sleep(a.off)
    tmux("clear-history", "-t", SESSION)
    base = counts([OURS, FACTORY, r"x0=0x0\b", r"root@dmc12[^\n]*#", BAD])
    power(a, "on")
    return wait_boot(a, base, timeout, want_ours)


def smoke(a, st):
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        try:
            if run(reimage.SSH + [f"root@{a.ip}", "ls /dev/P* | wc -l"], 40).stdout.strip() == "48":
                break
        except subprocess.TimeoutExpired:
            pass
        time.sleep(5)
    r = run([sys.executable, str(ROOT / "smoke.py"), "--ip", a.ip, "--json"], 600)
    res = json.loads(r.stdout) if r.stdout.strip().startswith("{") else {"pass": False, "results": []}
    st["smoke_failed"] = [c["check"] for c in res.get("results", []) if not c["pass"]]
    if not res.get("pass"):
        raise Fail(f"smoke failed: {st['smoke_failed']}")
    st["smoke"] = "pass"


def escape(a, st):
    """Power-cycle holding `u`: the factory U-Boot must come up, then boot normally again."""
    seen = stage3_u(a)
    if not {"user", "factory", "prompt"} <= seen or "ours" in seen:
        raise Fail(f"`u` escape not seen (new matches: {sorted(seen)})")
    identify(a, st)                      # a usable prompt, not just a banner
    if "mx25u12835f" not in con("sf probe 0:0").lower():
        raise Fail("escape: factory prompt up but sf probe failed")
    st["escape"] = "factory U-Boot via u, prompt + sf probe OK"
    st["boot_after_escape"] = power_cycle(a, want_ours=True)


# ---- modes ------------------------------------------------------------------------------------
def slot_obj(img, n):
    """(object bytes, None) of IPXE n per the model, or (None, why)."""
    off, mx = IPXE_SLOTS[n]
    s, f = alboot.check_object(img, off, mx, IPXE)
    if s != "OK":
        return None, s
    return bytes(img[off:off + alboot.HDR + f["size"] + 4]), None


def booted(unit, data, app):
    """A journal record proving these bytes booted here with this BOOT_MODE app."""
    for r in journal(unit):
        if r.get("payload_sha256") != sha(data) or r.get("app") != hex(app) or r.get("failed"):
            continue
        if (r.get("kind") == "rehearsal" and r.get("pass")) or \
           (r.get("kind") in ("install", "update", "switch") and r.get("boot")):
            return r
    return None


def running_slot(a, img):
    """Power-cycle, stop our U-Boot at `=>`, CRC its loaded image at 0x08000000 against both IPXE
    objects, then `boot`. Both builds print the same banner, the bytes don't lie."""
    n_text = 0x60000          # text + rodata: not touched before relocation (bss overlays .rela)
    power(a, "off")
    time.sleep(a.off)
    tmux("clear-history", "-t", SESSION)
    base = counts([OURS])
    power(a, "on")
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        t = reimage.pane()
        if len(re.findall(OURS, t)) > base[OURS] and "Hit any key" in t[t.rfind("U-Boot 2026"):]:
            break
        time.sleep(0.1)
    else:
        raise Fail("our U-Boot did not come up")
    tmux("send-keys", "-t", SESSION, "-l", " ")
    deadline = time.monotonic() + 10
    while not reimage.pane().rstrip().endswith("=>") and time.monotonic() < deadline:
        time.sleep(0.2)
    got = uartboot.crc_of(reimage.console(f"crc32 {PAYLOAD_LOAD:#x} {n_text:#x}", prompt="=>"))
    which = "unknown"
    for n in IPXE_SLOTS:
        obj, _ = slot_obj(img, n)
        if obj and zlib.crc32(obj[alboot.HDR:alboot.HDR + n_text]) == got:
            which = f"ipxe{n}"
    tmux("send-keys", "-t", SESSION, "-l", "boot")
    tmux("send-keys", "-t", SESSION, "Enter")
    return which


def mode_slots(a, st, img, unknown):
    app = boot_mode_app(img)
    active = "factory (UBOOT 0)" if app == UBOOT_ID else \
        f"ipxe{app >> 28}" if app & 0x0FFFFFFF == IPXE else f"app {app:#x} (unknown)"
    st["boot_mode"] = f"app {app:#x} -> {active}"
    for oid, n in ((UBOOT_ID, 0), (UBOOT_ID, 1), (IPXE, 0), (IPXE, 1)):
        off, mx = uartboot.slot(img, oid, n)
        s, f = alboot.check_object(img, off, mx, oid)
        name = f"{alboot.NAMES[oid].lower()}{n}"
        if s != "OK":
            st[name] = f"@{off:#x} {mx // 1024} KiB: {s}"
            continue
        obj = bytes(img[off:off + alboot.HDR + f["size"] + 4])
        ver = re.search(rb"U-Boot 20\d\d\.\d\d[\w.-]*", obj)
        who = next((r for r in reversed(journal(a.unit)) if r.get("kind") == "write"
                    and r.get("phase") == "intent" and r.get("off") == off), None)
        st[name] = (f"@{off:#x} {mx // 1024} KiB: {ver.group().decode() if ver else '?'}, {len(obj):#x} B, "
                    f"{mx - len(obj)} B free, sha256 {sha(obj)[:16]}"
                    + (f", written {who['t'][:16]}" if who else ", factory")
                    + (" [ACTIVE]" if name == active or (oid == UBOOT_ID and n == 0 and app == UBOOT_ID) else ""))
    st["unknown"] = unknown
    last = next((r for r in reversed(journal(a.unit)) if r.get("cs0_after") or r.get("cs0")), None)
    st["last_full_read"] = f"{last['t'][:16]} {last.get('cs0_after') or last.get('cs0')}" if last else "none"
    if a.live:
        st["running"] = running_slot(a, img)


def mode_clipcheck(a, st, img, unknown):
    """Programmer dumps (box unpowered, 1.8 V, spi-recovery.md): equal to each other and to the
    model below the env; differing 4 KiB sectors are listed with the TOC object they belong to."""
    if not a.dump:
        raise Fail("clipcheck needs --dump FILE (twice: two reads must agree)")
    dumps = [alboot.load(str(d)) for d in a.dump]
    st["dumps"] = {str(d): sha(x) for d, x in zip(a.dump, dumps)}
    if any(len(x) != 0x1000000 for x in dumps):
        raise Fail(f"dump sizes {[len(x) for x in dumps]}, expected 16 MiB")
    if len(set(map(sha, dumps))) != 1:
        raise Fail("the dumps differ from each other: re-seat the clip and read again")
    d = dumps[0]
    names = [(off, mx, f"{alboot.NAMES.get(i, hex(i))} {n}") for i, n, off, mx in uartboot.toc_entries(img)]
    bad = {}
    for lo in range(0, MODEL_END, SECTOR):
        if any(overlaps(u, lo, lo + SECTOR) for u in unknown):
            continue
        if d[lo:lo + SECTOR] != img[lo:lo + SECTOR]:
            where = next((nm for off, mx, nm in names if off <= lo < off + mx), "outside the TOC")
            bad.setdefault(where, []).append(hex(lo))
    st["env_sha256"] = sha(d[MODEL_END:])
    if bad:
        bad = {k: f"{len(v)} sector(s) {v[0]}..{v[-1]}" for k, v in bad.items()}
        raise Fail(f"dump != model: {bad}")
    st["clipcheck"] = f"0..{MODEL_END:#x} == model ({read_hint(img, unknown)}); env not modelled"


def read_hint(img, unknown):
    return f"crc32 {zlib.crc32(img):08x}" if not unknown else f"unknown {unknown} skipped"


def mode_status(a, st, img, unknown):
    to_factory(a, img, unknown)
    identify(a, st)
    state_checks(a, st)
    st["cs0"] = read_cs0(a, img, unknown, "now")
    st["unknown"] = unknown
    st["boot_mode_app"] = hex(boot_mode_app(img))
    for n, (off, mx) in IPXE_SLOTS.items():
        s, f = alboot.check_object(img, off, mx, IPXE)
        st[f"ipxe{n}"] = s if s != "OK" else f"OK sha256 {sha(bytes(img[off:off + alboot.HDR + f['size'] + 4]))[:16]}"
    # controls: the factory validator accepts a good object and rejects a blank slot
    validate(a, UBOOT_ID, 0, "UBOOT 0 (control)")
    for n, (off, mx) in IPXE_SLOTS.items():
        if img[off:off + mx] == b"\xff" * mx:
            validate(a, IPXE, n, f"blank IPXE {n} (negative control)", expect=False)
            break


def mode_rehearse(a, st, img, unknown):
    if unknown:
        raise Fail(f"unknown flash ranges {unknown}: run status/rollback first")
    inst, app, data, bm = plan(a, img)
    st.update(inst=inst, app=hex(app), payload_sha256=sha(data), bootmode_sha256=sha(bm))
    ram = bytearray(img[:uartboot.IMG_SIZE])
    for n in (0, 1):
        uartboot.put(ram, img, 6, n, b"")        # UBOOT_SCRIPT blank (RAM-load mode would run it)
    uartboot.put(ram, img, IPXE, inst, data)
    uartboot.put(ram, img, BOOT_MODE, 0, bm)
    work = ROOT / "dumps" / a.unit / "rehearsal"
    work.mkdir(parents=True, exist_ok=True)
    image = work / f"spi-upgrade-{sha(bytes(ram))[:12]}.bin"
    image.write_bytes(ram)
    so, _ = uartboot.slot(ram, 4, 0)
    s3 = alboot.check_object(ram, so, 0)[1]["size"]
    stg3 = bytes(ram[so + alboot.HDR:so + alboot.HDR + s3])
    meta = {"crc": zlib.crc32(ram), "stg3_src": uartboot.IMG_ADDR + so + alboot.HDR,
            "stg3_size": len(stg3), "stg3_crc": zlib.crc32(stg3)}
    image.with_suffix(".txt").write_text("".join(f"{k}={v:#x}\n" for k, v in meta.items()))
    ra = [sys.executable, str(ROOT / "uartboot.py"), "route-a", "--image", str(image), "--run",
          "--no-agent-trace", "--server", a.server, "--tftp-root", str(a.tftp_root)]
    tdir = a.tftp_root / "dmc12" / "route-a"
    try:
        for key in (None, "u"):
            to_factory(a, img, unknown)
            identify(a, st)
            pats = [OURS, FACTORY, r"x0=0x0\b", r"root@dmc12[^\n]*#", USER_U, BAD]
            tmux("clear-history", "-t", SESSION)
            base = counts(pats)
            r = run(ra + (["--key", key] if key else []), 600)
            if r.returncode:
                raise Fail(f"route-a failed: {r.stdout[-300:]}{r.stderr[-300:]}")
            go = f"{PROMPT} go {uartboot.TRAMP_ADDR:#x}"
            if key is None:
                st["boot"] = wait_boot(a, base, 300, want_ours=True, after=go)
                smoke(a, st)
                st["pass"] = True
            else:
                deadline = time.monotonic() + 120
                while time.monotonic() < deadline:
                    t = since(go)
                    if all(re.search(p, t) for p in (USER_U, FACTORY)):
                        st["escape_pass"] = True
                        break
                    time.sleep(0.5)
                else:
                    raise Fail("Route A with `u`: no 'User request for loading U-Boot' + factory banner")
            power(a, "off")
            time.sleep(max(a.off, 12))           # AC off >= 10 s after every Route A run
    finally:
        for p in tdir.glob("spi-upgrade-*"):     # vendor bytes: never left on the TFTP server
            p.unlink()
    power(a, "on")
    st["image"] = str(image.relative_to(ROOT))


def mode_write(a, st, img, unknown):
    bm_lo, bm_hi = BOOT_MODE_OFF, BOOT_MODE_OFF + SECTOR
    if a.mode == "switch" and a.to == "factory":
        a.mode = "rollback"
    if a.mode == "rollback":
        targets = [(bm_lo, bm_hi)]
        if not any(overlaps(u, bm_lo, bm_hi) for u in unknown) and boot_mode_app(img) == UBOOT_ID:
            raise Fail("BOOT_MODE is already blank/invalid: stage3 boots the factory U-Boot")
    elif a.mode == "switch":
        if any(overlaps(u, bm_lo, bm_hi) for u in unknown):
            raise Fail("BOOT_MODE is an unknown range: `switch --to factory` (rollback) first")
        inst = int(a.to[-1])
        data, why = slot_obj(img, inst)
        if data is None:
            raise Fail(f"IPXE {inst} holds no valid object ({why}): nothing to switch to")
        app = IPXE | inst << 28
        if boot_mode_app(img) == app:
            raise Fail(f"BOOT_MODE already selects IPXE {inst}")
        bm = boot_mode_obj(app)
        targets = [(bm_lo, bm_hi)]
        st.update(inst=inst, app=hex(app), payload_sha256=sha(data), bootmode_sha256=sha(bm))
    else:
        inst, app, data, bm = plan(a, img)
        off, mx = IPXE_SLOTS[inst]
        targets = [(off, off + mx), (bm_lo, bm_hi)]
        st.update(inst=inst, app=hex(app), payload_sha256=sha(data), bootmode_sha256=sha(bm))
        stray = [u for u in unknown if not any(t[0] <= u[0] and u[1] <= t[1] for t in targets)]
        if stray:
            raise Fail(f"unknown flash ranges {stray} outside this run's targets: status/rollback first")
    open_, over = gates(a)
    blockers = [] if a.mode == "rollback" else [f"gate {g} open in {GATES.name}" for g in open_]
    if a.mode == "switch":
        if not (booted(a.unit, data, app) or rehearsed(a.unit, data, bm)):
            blockers.append(f"IPXE {inst} never booted here with app {app:#x} and wasn't rehearsed so")
    elif a.mode != "rollback" and not rehearsed(a.unit, data, bm):
        blockers.append("no passing rehearsal (boot + escape) of these exact bytes on this unit")
    st["gate_overrides"] = over
    if blockers and a.write:
        raise Fail("; ".join(blockers) + " (spi-upgrade-plan.md preconditions; --override-gate NAME=REASON)")
    if blockers:
        st["would_refuse"] = blockers
    to_factory(a, img, unknown)
    identify(a, st)
    state_checks(a, st)
    st["cs0_before"] = read_cs0(a, img, unknown, "before")
    rest = [u for u in unknown if not any(t[0] <= u[0] and u[1] <= t[1] for t in targets)]
    d = a.tftp_root / a.tftp_dir
    try:
        if a.mode == "rollback":
            new = write_region(a, img, bm_lo, SECTOR, b"", 0, "erase BOOT_MODE")
        elif a.mode == "switch":
            stage(a, {"spi-upgrade-bootmode.obj": bm})
            tftp(a, A_BOOTMODE, "spi-upgrade-bootmode.obj", bm)
            try:
                new = write_region(a, img, bm_lo, SECTOR, bm, A_BOOTMODE, f"BOOT_MODE app {app:#x}")
                st["bootmode_live"] = a.write
            except Fail as e:
                if a.write:
                    new = bootmode_clear(a, img, str(e))
                    st["bootmode_recovered"] = True
                raise
        else:
            stage(a, {"spi-upgrade-ipxe.fimg": data, "spi-upgrade-bootmode.obj": bm})
            tftp(a, A_PAYLOAD, "spi-upgrade-ipxe.fimg", data)
            tftp(a, A_BOOTMODE, "spi-upgrade-bootmode.obj", bm)
            # The whole inactive slot is erased: no stale tail from an older, longer build.
            # Harmless if it fails: BOOT_MODE does not point at this instance yet.
            new = write_region(a, img, off, mx, data, A_PAYLOAD, f"IPXE {inst}")
            if a.write:
                validate(a, IPXE, inst, f"IPXE {inst}")
            try:
                # No factory validate here: its load address is 0 and what the validator does with
                # that is unknown; read-back crc + the rehearsal of these exact bytes cover it.
                new = write_region(a, new, bm_lo, SECTOR, bm, A_BOOTMODE, f"BOOT_MODE app {app:#x}")
                st["bootmode_live"] = a.write
            except Fail as e:
                if a.write:
                    new = bootmode_clear(a, new, str(e))
                    st["bootmode_recovered"] = True
                raise
    finally:
        for p in d.glob("spi-upgrade-*"):
            p.unlink()
    if not a.write:
        st["dry_run"] = True
        return
    st["cs0_after"] = read_cs0(a, new, rest, "after")   # nothing outside the targets changed
    st["boot"] = power_cycle(a, want_ours=a.mode != "rollback")
    if a.ip:
        smoke(a, st)
    if a.mode != "rollback":
        st["running"] = running_slot(a, new)        # which IPXE copy really runs
        if st["running"] != f"ipxe{st['inst']}":
            raise Fail(f"BOOT_MODE selects IPXE {st['inst']} but {st['running']} is running")
        wait_boot(a, {}, 300, want_ours=True, after="=> boot")
        escape(a, st)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("mode", choices=["status", "slots", "rehearse", "clipcheck", *WRITE_MODES])
    p.add_argument("--to", choices=["ipxe0", "ipxe1", "factory"], help="switch: the slot to boot")
    p.add_argument("--live", action="store_true", help="slots: also prove which slot runs (power-cycles)")
    p.add_argument("--unit", required=True)
    p.add_argument("--plug", type=int, help="lab-power plug of this unit (all modes but clipcheck)")
    p.add_argument("--dump", type=pathlib.Path, action="append", default=[],
                   help="clipcheck: a programmer dump of CS0 (.bin or .zst); give two reads")
    p.add_argument("--ip", help="NixOS address for the smoke test (required for rehearse/install/update)")
    p.add_argument("--payload", type=pathlib.Path,
                   default=ROOT / "nixos-build/result-uboot-dmc12-stage3app/u-boot-ipxe-08000000.fimg")
    p.add_argument("--write", action="store_true", help="really write SPI (default: dry run)")
    p.add_argument("--yes", action="store_true", help="no unit-id prompts (agents)")
    p.add_argument("--override-gate", action="append", default=[], metavar="NAME=REASON")
    p.add_argument("--server", default="10.60.2.208")
    p.add_argument("--tftp-root", type=pathlib.Path, default=pathlib.Path("/srv/tftpboot"))
    p.add_argument("--tftp-dir", default="dmc12")
    p.add_argument("--off", type=int, default=12, help="seconds AC off per power cycle")
    p.add_argument("--read-timeout", type=int, default=600, help="seconds for a 16 MiB factory sf read")
    p.add_argument("--json", action="store_true")
    a = p.parse_args()
    if a.mode == "switch" and not a.to:
        p.error("switch needs --to ipxe0|ipxe1|factory")
    if a.mode not in ("clipcheck", "slots") and a.plug is None or (a.mode == "slots" and a.live and a.plug is None):
        p.error(f"{a.mode} needs --plug")
    if a.mode in ("rehearse", "install", "update") and not a.ip:
        p.error(f"{a.mode} needs --ip (smoke test after boot)")
    for o in a.override_gate:
        if "=" not in o or o.split("=")[0] not in GATE_NAMES:
            p.error(f"--override-gate NAME=REASON, NAME in {GATE_NAMES}")
    st = {"unit": a.unit, "mode": a.mode, "write": a.write}
    try:
        img, unknown = model(a.unit)
        check_layout(img)
        {"status": mode_status, "rehearse": mode_rehearse, "clipcheck": mode_clipcheck,
         "slots": mode_slots}.get(
            a.mode, mode_write)(a, st, img, unknown)
    except KeyboardInterrupt:
        st["failed"] = "interrupted. " + (DONT_POWER_OFF if a.write else "")
    except (Fail, subprocess.SubprocessError, OSError, KeyError, ValueError) as e:
        st["failed"] = f"{type(e).__name__}: {e}"
    except SystemExit as e:             # uartboot/alboot helpers raise SystemExit(str)
        if not isinstance(e.code, str):
            raise
        st["failed"] = e.code
    if "failed" in st and st.get("bootmode_live") and a.mode != "rollback":
        st["hint"] = "BOOT_MODE is live: power-cycle with `u` (stage3keys.py --key u), then `rollback --write`"
    if a.mode == "rehearse" or a.write:
        log(a.unit, {"kind": "rehearsal" if a.mode == "rehearse" else a.mode,
                     **{k: v for k, v in st.items() if k != "unit"}})
    print(json.dumps(st) if a.json else "\n".join(f"  {k}: {v}" for k, v in st.items()))
    return 1 if "failed" in st else 0


if __name__ == "__main__":
    sys.exit(main())
