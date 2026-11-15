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

Nothing is written without --write: every read-only step runs and the write commands are printed.
With --write each write asks for the unit id (--yes: no questions, for agents).

  spi-upgrade.py --unit u01 --plug 2 status
  spi-upgrade.py --unit u01 --plug 2 rehearse --ip 10.60.2.10
  spi-upgrade.py --unit u01 --plug 2 install --ip 10.60.2.10           # dry run
  spi-upgrade.py --unit u01 --plug 2 install --ip 10.60.2.10 --write

Host model of CS0 = dumps/<unit>/mtd0.bin (verified against its manifest) with every write in
inventory/<unit>/spi-upgrade.jsonl replayed ("kind": "write", journaled *before* the command is
sent; the written bytes are kept in dumps/<unit>/spi-writes/). Every flash read is compared with
the model, so a torn or unexpected write stops the script. Only 0..0xfd0000 is modelled: the env
pair above it changes with every saveenv. Never `saveenv`/`env default`/`delenv` here.
"""
import argparse, hashlib, json, pathlib, re, struct, subprocess, sys, time, zlib

ROOT = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
import alboot     # noqa: E402
import reimage    # noqa: E402  (console, pane, manifest + identity helpers)
import uartboot   # noqa: E402  (TOC slots, object wrapper, Route A)

Fail = reimage.Fail
MODEL_END = 0xFD0000                     # env pair 0xfd0000..0xffffff is not modelled
BOOT_MODE_OFF, SECTOR = 0x340000, 0x1000
IPXE, BOOT_MODE, UBOOT_ID = 0x110, 0, 5
PAYLOAD_LOAD = 0x08000000                # IPXE object: load = exec (stage3 jumps there at EL2)
# Factory U-Boot RAM staging (below its relocated self, above the 0x01xxxxxx stage3 area).
A_PAYLOAD, A_BOOTMODE, A_READBACK, A_CS0 = 0x09000000, 0x09F00000, 0x0C000000, 0x0A000000
OURS = r"U-Boot 2026\.10-dmc12-selfinit"
FACTORY = r"U-Boot 2015\.07"
GATES = ROOT / "spi-upgrade-gates.json"
GATE_NAMES = ("clip_read", "review", "gpio22", "cold_pcie")
WRITE_MODES = ("install", "update", "rollback")


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
    """Expected CS0 0..MODEL_END: the verified backup plus every journaled write."""
    have, missing = reimage.manifest_ok(unit)
    if "mtd0.bin" in missing:
        raise Fail(f"no verified mtd0.bin for {unit} in dumps/{unit}/manifest.jsonl (acquire.py first)")
    raw = alboot.load(str(ROOT / "dumps" / unit / "mtd0.bin.zst"))
    if sha(raw) != have["mtd0.bin"]["sha256"]:
        raise Fail(f"dumps/{unit}/mtd0.bin.zst does not match its manifest sha256")
    img = bytearray(raw[:MODEL_END])
    for r in journal(unit):
        if r.get("kind") != "write":
            continue
        off = r["off"]
        if "erase" in r:
            img[off:off + r["erase"]] = b"\xff" * r["erase"]
        else:
            data = (ROOT / r["file"]).read_bytes()
            if sha(data) != r["sha256"]:
                raise Fail(f"{r['file']} does not match its journal sha256")
            img[off:off + len(data)] = data
    return img


def boot_mode_app(img):
    """App stage3 will load per the model (5 = factory U-Boot when BOOT_MODE is blank/invalid)."""
    off, mx = uartboot.slot(img, BOOT_MODE, 0)
    st, f = alboot.check_object(img, off, mx, BOOT_MODE)
    if st != "OK":
        return UBOOT_ID
    return struct.unpack_from("<I", img, off + alboot.HDR + 4)[0]


def boot_mode_obj(app):
    # 0x40 B of data like the factory object (0x8c B wrapped); stage3 reads word +4 only.
    return uartboot.obj(BOOT_MODE, 0, struct.pack("<II", 0, app).ljust(0x40, b"\0"))


def check_layout(img):
    """No unknown (e.g. SIG/PUK) objects; BOOT_MODE and both IPXE slots where we expect them."""
    unknown = [hex(i) for i, n, off, mx in uartboot.toc_entries(img) if i not in alboot.NAMES]
    if unknown:
        raise Fail(f"TOC has unknown object ids {unknown}: not a layout this script knows")
    off, mx = uartboot.slot(img, BOOT_MODE, 0)    # 64 KiB slot; only its first sector is used
    if off != BOOT_MODE_OFF or mx < SECTOR:
        raise Fail(f"BOOT_MODE slot is {off:#x}/{mx:#x}, expected 0x340000 and >= 4 KiB")
    for n in (0, 1):
        off, mx = uartboot.slot(img, IPXE, n)
        if off % SECTOR or mx % SECTOR or off + mx > MODEL_END:
            raise Fail(f"IPXE {n} slot {off:#x}/{mx:#x} is not sector aligned")


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
    off, mx = uartboot.slot(img, IPXE, inst)
    if len(data) > mx:
        raise Fail(f"payload {len(data):#x} B does not fit IPXE {inst} ({mx:#x} B)")
    app = IPXE | inst << 28
    return inst, app, data, boot_mode_obj(app)


def expected_after(img, writes):
    out = bytearray(img)
    for off, data, erase in writes:
        if erase:
            out[off:off + erase] = b"\xff" * erase
        else:
            out[off:off + len(data)] = data
    return out


# ---- gates --------------------------------------------------------------------------------
def gates(a):
    g = json.loads(GATES.read_text()) if GATES.exists() else {}
    over = dict(o.split("=", 1) for o in a.override_gate)
    open_ = [n for n in GATE_NAMES if not g.get(n) and n not in over]
    return open_, over


def rehearsed(unit, data, bm):
    for r in journal(unit):
        if (r.get("kind") == "rehearsal" and r.get("pass") and r.get("escape_pass")
                and r.get("payload_sha256") == sha(data) and r.get("bootmode_sha256") == sha(bm)):
            return r
    return None


# ---- console --------------------------------------------------------------------------------
def con(cmd, timeout=30):
    return reimage.console(cmd, timeout=timeout)


def crc(cmd_addr, size, timeout=120):
    return uartboot.crc_of(con(f"crc32 {cmd_addr:#x} {size:#x}", timeout))


def run(cmd, timeout):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)


def power(a, state):
    run([sys.executable, str(ROOT.parents[1] / "tools" / "lab_power.py"), str(a.plug), state], 30)


def counts(pats):
    t = reimage.pane()
    return {p: len(re.findall(p, t)) for p in pats}


def to_factory(a, img):
    """Stop at the factory prompt. With our U-Boot selected, the `u` key is the only way there."""
    if boot_mode_app(img) == UBOOT_ID:
        r = run([sys.executable, str(ROOT / "powercycle_to_uboot.py"), "--plug", str(a.plug)], 240)
        ok = "ALPINE_DB>" in r.stdout
    else:
        r = run([sys.executable, str(ROOT / "stage3keys.py"), "--plug", str(a.plug), "--key", "u",
                 "--off", str(a.off)], 240)
        ok = "User request" in r.stdout and reimage.pane().rstrip().endswith("ALPINE_DB>")
    if r.returncode or not ok:
        raise Fail(f"did not reach the factory prompt: {r.stdout[-300:]}{r.stderr[-300:]}")


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
    otp = con("otp show direct")
    if "- Secure boot: disabled" not in otp:
        raise Fail("OTP does not say 'Secure boot: disabled': stop (spi-uboot-replacement.md §6)")
    st["otp"] = "secure boot disabled"


def read_cs0(a, img, what):
    """sf probe + full read of 0..MODEL_END; crc32 must equal the model."""
    if "mx25u12835f" not in con("sf probe 0:0").lower():
        raise Fail("sf probe 0:0 did not detect the MX25U12835F")
    con(f"sf read {A_CS0:#x} 0 {MODEL_END:#x}", a.read_timeout)
    got, want = crc(A_CS0, MODEL_END, 300), zlib.crc32(img)
    if got != want:
        raise Fail(f"CS0 {what}: crc32 {got:08x} != model {want:08x}. STOP: the flash is not what the "
                   "journal says (spi-upgrade-plan.md, failure handling)")
    return got


# ---- steps ----------------------------------------------------------------------------------
def stage(a, files):
    d = a.tftp_root / a.tftp_dir
    d.mkdir(parents=True, exist_ok=True)
    for name, data in files.items():
        (d / name).write_bytes(data)
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


def write_region(a, img, off, data, ram, label, erase=None):
    """Erase the region's sectors, write `data` from RAM `ram`, read back, crc vs host."""
    erase = erase or -(-len(data) // SECTOR) * SECTOR
    cmds = [f"sf erase {off:#x} {erase:#x}"] + ([f"sf write {ram:#x} {off:#x} {len(data):#x}"] if data else [])
    if not a.write:
        say(a, f"dry run, would send ({label}): " + "; ".join(cmds))
        return expected_after(img, [(off, b"", erase), (off, data, 0)])
    confirm(a, f"[WRITES SPI] {label}: {'; '.join(cmds)}")
    log(a.unit, {"kind": "write", "label": label, "off": off, "erase": erase})
    con(cmds[0], 120)
    if data:
        log(a.unit, {"kind": "write", "label": label, "off": off, "sha256": sha(data), "file": keep(a, data)})
        con(cmds[1], 120)
    new = model(a.unit)
    con(f"sf read {A_READBACK:#x} {off:#x} {erase:#x}", 120)
    if crc(A_READBACK, erase) != zlib.crc32(new[off:off + erase]):
        raise Fail(f"{label}: read-back crc32 differs from what was written. STOP")
    return new


def validate(a, oid, inst, label, control=False):
    if not a.write and not control:      # nothing was written: the slot is still blank
        return
    out = con(f"flash_contents_obj_validate {alboot.NAMES[oid]} {inst} && echo VALID-OK || echo VALID-FAIL")
    if "VALID-OK" not in out or "failed" in out:
        raise Fail(f"{label}: factory flash_contents_obj_validate rejects it: {out.strip()[-200:]}")


def wait_boot(a, base, timeout, want_ours=True):
    """After a power-on: which U-Boot came up, then the NixOS login prompt."""
    pats = {"ours": OURS, "factory": FACTORY, "x0": r"x0=0x0\b", "login": r"root@dmc12[^\n]*#",
            "bad": r"Kernel panic|Synchronous Abort|### ERROR|Unknown command|Wrong Image"}
    t0, seen = time.monotonic(), {}
    while time.monotonic() - t0 < timeout:
        t = reimage.pane()
        for k, p in pats.items():
            if k not in seen and len(re.findall(p, t)) > base.get(p, 0):
                seen[k] = round(time.monotonic() - t0, 1)
        if "bad" in seen or "login" in seen:
            break
        time.sleep(0.5)
    if "bad" in seen:
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
    subprocess.run(["tmux", "clear-history", "-t", reimage.SESSION], check=True)
    base = counts([OURS, FACTORY, r"x0=0x0\b", r"root@dmc12[^\n]*#",
                   r"Kernel panic|Synchronous Abort|### ERROR|Unknown command|Wrong Image"])
    power(a, "on")
    return wait_boot(a, base, timeout, want_ours)


def smoke(a, st):
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        r = run(reimage.SSH + [f"root@{a.ip}", "ls /dev/P* | wc -l"], 40)
        if r.stdout.strip() == "48":
            break
        time.sleep(5)
    r = run([sys.executable, str(ROOT / "smoke.py"), "--ip", a.ip, "--json"], 600)
    res = json.loads(r.stdout) if r.stdout.strip().startswith("{") else {"pass": False, "results": []}
    st["smoke_failed"] = [c["check"] for c in res.get("results", []) if not c["pass"]]
    if not res.get("pass"):
        raise Fail(f"smoke failed: {st['smoke_failed']}")
    st["smoke"] = "pass"


def escape(a, st):
    """Power-cycle holding `u`: the factory U-Boot must come up, then boot normally again."""
    r = run([sys.executable, str(ROOT / "stage3keys.py"), "--plug", str(a.plug), "--key", "u",
             "--off", str(a.off)], 240)
    if "User request for loading U-Boot" not in r.stdout:
        raise Fail(f"`u` escape not seen: {r.stdout[-400:]}")
    st["escape"] = "factory U-Boot via u"
    st["boot_after_escape"] = power_cycle(a, want_ours=True)


# ---- modes ------------------------------------------------------------------------------------
def mode_status(a, st, img):
    to_factory(a, img)
    identify(a, st)
    state_checks(a, st)
    st["cs0_crc"] = f"{read_cs0(a, img, 'now'):08x}"
    st["boot_mode_app"] = hex(boot_mode_app(img))
    for n in (0, 1):
        off, mx = uartboot.slot(img, IPXE, n)
        s, f = alboot.check_object(img, off, mx, IPXE)
        st[f"ipxe{n}"] = s if s != "OK" else f"OK sha256 {sha(bytes(img[off:off + alboot.HDR + f['size'] + 4]))[:16]}"
    # control: the factory validator on a known-good object, so a later IPXE failure means something
    validate(a, UBOOT_ID, 0, "UBOOT 0 (control)", control=True)


def mode_rehearse(a, st, img):
    inst, app, data, bm = plan(a, img)
    st.update(inst=inst, app=hex(app), payload_sha256=sha(data), bootmode_sha256=sha(bm))
    ram = bytearray(img[:uartboot.IMG_SIZE])
    ram.extend(b"\xff" * (uartboot.IMG_SIZE - len(ram)))
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
            to_factory(a, img)
            identify(a, st)
            pats = [OURS, FACTORY, r"x0=0x0\b", r"root@dmc12[^\n]*#", "User request for loading U-Boot",
                    r"Kernel panic|Synchronous Abort|### ERROR|Unknown command|Wrong Image"]
            subprocess.run(["tmux", "clear-history", "-t", reimage.SESSION], check=True)
            base = counts(pats)
            r = run(ra + (["--key", key] if key else []), 600)
            if r.returncode:
                raise Fail(f"route-a failed: {r.stdout[-300:]}{r.stderr[-300:]}")
            if key is None:
                st["boot"] = wait_boot(a, base, 300, want_ours=True)
                smoke(a, st)
                st["pass"] = True
            else:
                deadline = time.monotonic() + 120
                while time.monotonic() < deadline:
                    c = counts(pats)
                    if all(c[p] > base[p] for p in ("User request for loading U-Boot", FACTORY)):
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


def mode_write(a, st, img):
    if a.mode == "rollback":
        if boot_mode_app(img) == UBOOT_ID:
            raise Fail("BOOT_MODE is already blank/invalid: stage3 boots the factory U-Boot")
    else:
        inst, app, data, bm = plan(a, img)
        st.update(inst=inst, app=hex(app), payload_sha256=sha(data), bootmode_sha256=sha(bm))
    open_, over = gates(a)
    blockers = [f"gate {g} open in {GATES.name}" for g in open_]
    if a.mode != "rollback" and not rehearsed(a.unit, data, bm):
        blockers.append("no passing rehearsal (boot + escape) of these exact bytes on this unit")
    st["gate_overrides"] = over
    if blockers and a.write:
        raise Fail("; ".join(blockers) + " (spi-upgrade-plan.md preconditions; --override-gate NAME=REASON)")
    if blockers:
        st["would_refuse"] = blockers
    to_factory(a, img)
    identify(a, st)
    state_checks(a, st)
    st["cs0_before"] = f"{read_cs0(a, img, 'before'):08x}"
    if a.mode == "rollback":
        new = write_region(a, img, BOOT_MODE_OFF, b"", 0, "erase BOOT_MODE", erase=SECTOR)
    else:
        off, mx = uartboot.slot(img, IPXE, inst)
        stage(a, {"spi-upgrade-ipxe.fimg": data, "spi-upgrade-bootmode.obj": bm})
        tftp(a, A_PAYLOAD, "spi-upgrade-ipxe.fimg", data)
        tftp(a, A_BOOTMODE, "spi-upgrade-bootmode.obj", bm)
        # the whole inactive slot is erased: no stale tail from an older, longer build
        new = write_region(a, img, off, data, A_PAYLOAD, f"IPXE {inst}", erase=mx)
        validate(a, IPXE, inst, f"IPXE {inst}")
        new = write_region(a, new, BOOT_MODE_OFF, bm, A_BOOTMODE, f"BOOT_MODE app {app:#x}", erase=SECTOR)
        validate(a, BOOT_MODE, 0, "BOOT_MODE")
        for p in (a.tftp_root / a.tftp_dir).glob("spi-upgrade-*"):
            p.unlink()
    if not a.write:
        st["dry_run"] = True
        return
    # The new model differs from the old only in the written regions (by construction); the
    # full re-read proves the flash agrees and nothing else changed.
    st["cs0_after"] = f"{read_cs0(a, new, 'after'):08x}"
    st["boot"] = power_cycle(a, want_ours=a.mode != "rollback")
    if a.ip:
        smoke(a, st)
    if a.mode != "rollback":
        escape(a, st)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("mode", choices=["status", "rehearse", *WRITE_MODES])
    p.add_argument("--unit", required=True)
    p.add_argument("--plug", type=int, required=True, help="lab-power plug of this unit")
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
    if a.mode in ("rehearse", "install", "update") and not a.ip:
        p.error(f"{a.mode} needs --ip (smoke test after boot)")
    for o in a.override_gate:
        if "=" not in o or o.split("=")[0] not in GATE_NAMES:
            p.error(f"--override-gate NAME=REASON, NAME in {GATE_NAMES}")
    st = {"unit": a.unit, "mode": a.mode, "write": a.write}
    try:
        img = model(a.unit)
        check_layout(img)
        {"status": mode_status, "rehearse": mode_rehearse}.get(a.mode, mode_write)(a, st, img)
    except (Fail, subprocess.TimeoutExpired, SystemExit) as e:
        if isinstance(e, SystemExit) and not isinstance(e.code, str):
            raise
        st["failed"] = str(e)
    if a.mode == "rehearse" or a.write:
        log(a.unit, {"kind": "rehearsal" if a.mode == "rehearse" else a.mode,
                     **{k: v for k, v in st.items() if k != "unit"}})
    print(json.dumps(st) if a.json else "\n".join(f"  {k}: {v}" for k, v in st.items()))
    return 1 if "failed" in st else 0


if __name__ == "__main__":
    sys.exit(main())
