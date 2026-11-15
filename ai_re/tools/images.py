#!/usr/bin/env python3
"""ai-re-images: catalog, tag, verify and inspect device images, read-only.

  ai-re-images scan [--device D] [--hash]       # manifests vs files: untracked, missing, conflicts
  ai-re-images list [filters] [--sort date|size|device|version|kind|unit|path] [-r]
  ai-re-images show REF
  ai-re-images tag REF TAG... | untag REF TAG... | note REF TEXT
  ai-re-images set REF [--kind K] [--unit U] [--version V]    # '' clears an override
  ai-re-images verify [REF...] [filters] [--raw]               # re-hash vs recorded
  ai-re-images inspect [REF...] [filters] [--deep] [--no-store]
  ai-re-images dupes
  ai-re-images export-qcow2 REF OUT.qcow2 [--scratch DIR]

Filters: --device, --unit, --kind, --tag (repeatable, all must match), --text,
--status. Every command takes --json. REF is a path (relative to the cwd, the
ai_re root or devices/), DEVICE:PATH, or a sha256 prefix (6+ hex digits).

Images live in devices/*/{dumps,dump,images,firmware,captures}. Recorded hashes
come from the files already there (manifest.json[l], acquisition/verification
JSON, SHA256SUMS, RAW-SHA256SUMS, *.sha256, device-sha256.txt) and are only
read. The tool's own additions (tags, notes, kind/unit/version overrides,
inspect results) go in devices/<device>/images.jsonl. Image files are never
written, moved or mounted; files with "private" in their path are skipped.

Exit codes: 0 ok, 1 mismatch/conflict/failure, 2 usage or unknown REF,
3 (verify) nothing mismatched but recorded files are missing.
"""

import argparse
import binascii
import contextlib
import datetime
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

SCAN_DIRS = ("dumps", "dump", "images", "firmware", "captures")
SIDECAR = "images.jsonl"
WRAPPERS = {".zst": "zstd", ".xz": "xz"}
ZSTD_D = ["zstd", "-dcq", "--long=27"]      # decompression is single-threaded
XZ_D = ["xz", "-dcq", "-T1"]
CHUNK = 1 << 20
HEAD = 1 << 20
EMBED_MAX = 256 << 20       # whole-content scan (magic map, fill) up to this raw size
STREAM_LIMIT = 1 << 30      # inspect reads at most this far into a compressed stream
# Extensions that make an unlisted file an image candidate.
IMAGE_EXT = {".img", ".bin", ".zst", ".xz", ".dtb", ".itb", ".fit", ".uimage", ".elf",
             ".dav", ".cpio", ".rom", ".ubi", ".sqfs", ".squashfs", ".jffs2", ".iso",
             ".qcow2", ".vhd", ".vmdk", ".fw", ".partial"}
# Never images, even when a SHA256SUMS lists them (logs, notes, archives of sources).
TEXT_EXT = {".txt", ".log", ".md", ".json", ".jsonl", ".html", ".htm", ".xml", ".sh", ".py",
            ".dts", ".csv", ".sum", ".pdf", ".zip", ".tar", ".tgz", ".so", ".exe", ".jpg",
            ".jpeg", ".png", ".conf", ".cfg", ".ini", ".yaml", ".yml", ".toml", ".pass",
            ".end", ".dat", ".soc", ".bcm", ".264", ".cls", ".js", ".css"}
# Extraction trees and workspaces under the scan dirs: never walked.
SKIP_DIRS = {"open-jffs2", "p4full", "sd-files", "rootfs_extract", "sb_rootfs", "squashfs-root",
             "analysis", "ghidra", "__pycache__", ".git", "private"}
TAG_RE = re.compile(r"^[a-z0-9][a-z0-9._+-]*$")
VERSION_RE = re.compile(r"(?<![\d.])(\d+(?:\.\d+){2,})(?![\d])")


class Fail(Exception):
    def __init__(self, msg, code=2):
        super().__init__(msg)
        self.code = code


# ---------------------------------------------------------------- helpers

def is_private(rel):
    return "private" in str(rel).lower()


def split_wrapper(name):
    for ext, codec in WRAPPERS.items():
        if name.endswith(ext) and len(name) > len(ext):
            return name[:-len(ext)], codec
    return name, None


def ext_of(name):
    return os.path.splitext(name)[1].lower()


def human(n):
    if n is None:
        return "-"
    for unit in ("B", "K", "M", "G", "T"):
        if n < 1024 or unit == "T":
            return f"{n:.0f}{unit}" if unit == "B" or n >= 100 else f"{n:.1f}{unit}"
        n /= 1024


def today():
    return datetime.date.today().isoformat()


def find_root(arg=None):
    if arg:
        return Path(arg).resolve()
    if os.environ.get("AI_RE_ROOT"):
        return Path(os.environ["AI_RE_ROOT"]).resolve()
    for d in [Path.cwd(), *Path.cwd().parents]:
        for cand in (d, d / "ai_re"):
            if (cand / "devices").is_dir() and (cand / "DEVICES.md").is_file():
                return cand.resolve()
    raise Fail("can't find the ai_re root (run inside it, or set AI_RE_ROOT / --root)")


def cache_dir():
    base = os.environ.get("XDG_CACHE_HOME") or str(Path.home() / ".cache")
    return Path(base) / "ai_re-images"


def looks_text(head):
    if not head:
        return True
    printable = sum(1 for b in head if 32 <= b < 127 or b in (9, 10, 13))
    return printable / len(head) > 0.95


def stream_cmd(path, codec):
    return (ZSTD_D if codec == "zstd" else XZ_D) + [str(path)]


@contextlib.contextmanager
def stream(path, codec):
    """stdout of a decompressor reading PATH; killed on exit."""
    p = subprocess.Popen(stream_cmd(path, codec), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        yield p
    finally:
        if p.poll() is None:
            p.kill()
        p.wait()
        p.stdout.close()
        p.stderr.close()


def hash_file(path, progress=None):
    h, n = hashlib.sha256(), 0
    with open(path, "rb") as f:
        while chunk := f.read(CHUNK):
            h.update(chunk)
            n += len(chunk)
            if progress:
                progress(n)
    return h.hexdigest(), n


def hash_stream(path, codec, sink=None, progress=None):
    """sha256 and length of the decompressed content; optional sink(chunk)."""
    h, n = hashlib.sha256(), 0
    with stream(path, codec) as p:
        while chunk := p.stdout.read(CHUNK):
            h.update(chunk)
            n += len(chunk)
            if sink:
                sink(chunk)
            if progress:
                progress(n)
        err = p.stderr.read().decode(errors="replace").strip()
        rc = p.wait()
    if rc != 0:
        raise Fail(f"{codec} failed on {path}: {err[-300:]}", 1)
    return h.hexdigest(), n


def progress_printer(label, total):
    if not sys.stderr.isatty() or (total or 0) < (512 << 20):
        return None
    state = {"next": 0}

    def show(n):
        if n >= state["next"]:
            pct = f" {100 * n / total:.0f}%" if total else ""
            print(f"\r  {label}: {human(n)}{pct}   ", end="", file=sys.stderr, flush=True)
            state["next"] = n + (256 << 20)
    return show


# ---------------------------------------------------------------- hash cache (git-ignored: outside the repo)

class HashCache:
    def __init__(self):
        self.path = cache_dir() / "hashes.json"
        try:
            self.data = json.loads(self.path.read_text())
        except (OSError, ValueError):
            self.data = {}
        self.dirty = False

    def get(self, path, st):
        v = self.data.get(str(path))
        if v and v.get("size") == st.st_size and v.get("mtime_ns") == st.st_mtime_ns:
            return v.get("sha256")
        return None

    def put(self, path, st, sha):
        self.data[str(path)] = {"size": st.st_size, "mtime_ns": st.st_mtime_ns, "sha256": sha}
        self.dirty = True

    def save(self):
        if not self.dirty:
            return
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(".tmp")
        tmp.write_text(json.dumps(self.data, indent=0, sort_keys=True))
        os.replace(tmp, self.path)


# ---------------------------------------------------------------- catalog

def new_part():
    return {"name": None, "sha256": None, "bytes": None, "present": False, "size": None,
            "mtime": None, "device_sha256": None}


class Catalog:
    def __init__(self, root, devices=None, cache=None):
        self.root = root
        self.cache = cache or HashCache()
        self.entries = []
        self.conflicts = []
        self.warnings = []
        self.orphans = []
        self.private_skipped = 0
        self.sidecars = {}
        base = root / "devices"
        names = sorted(p.name for p in base.iterdir() if p.is_dir() and not p.name.startswith(("_", ".")))
        if devices:
            unknown = set(devices) - set(names)
            if unknown:
                raise Fail(f"unknown device: {', '.join(sorted(unknown))}")
            names = [n for n in names if n in devices]
        for dev in names:
            self._load_device(dev)

    # -- loading
    def _load_device(self, dev):
        ddir = self.root / "devices" / dev
        entries = {}
        self._cur = (dev, ddir, entries)
        files = []
        for sd in SCAN_DIRS:
            top = ddir / sd
            if top.is_dir():
                self._walk(top, files)
        manifests = [f for f in files if self._is_manifest(f.name)]
        for m in sorted(manifests):
            self._read_manifest(m)
        for f in files:
            if f in manifests:
                continue
            rel = f.relative_to(ddir)
            key = self._key(rel.parent.as_posix(), rel.name)
            known = key in entries
            if not known and not self._is_candidate(f):
                continue
            e, codec = self._get(rel.parent.as_posix(), rel.name)
            part = e["wrapped"] if codec else e["raw"]
            st = f.stat()
            part.update(name=rel.name, present=True, size=st.st_size, mtime=st.st_mtime)
            if codec:
                part["codec"] = codec
        sidecar = load_sidecar(ddir / SIDECAR)
        self.sidecars[dev] = sidecar
        for e in entries.values():
            self._finish(e)
        by_path = {e["lpath"]: e for e in entries.values()}
        for row in sidecar:
            e = by_path.get(row.get("path"))
            if e is None and row.get("sha256"):
                hits = [x for x in entries.values() if row["sha256"] in x["hashes"]]
                if len(hits) == 1:
                    e = hits[0]
                    self.warnings.append(f"{dev}: sidecar row {row['path']} matched by sha256 at {e['lpath']} (moved?)")
                    e["moved_from"] = row["path"]
            if e is None:
                self.orphans.append(f"{dev}/{row.get('path')}")
                continue
            e["side"] = row
            self._apply_side(e)
        self.entries.extend(sorted(entries.values(), key=lambda x: x["lpath"]))

    def _walk(self, top, out):
        for dirpath, dirnames, filenames in os.walk(top):
            d = Path(dirpath)
            keep = []
            for n in sorted(dirnames):
                sub = d / n
                if n in SKIP_DIRS or is_private(n) or sub.is_symlink() or looks_rootfs(sub):
                    continue
                keep.append(n)
            dirnames[:] = keep
            for n in sorted(filenames):
                p = d / n
                if is_private(n):
                    self.private_skipped += 1
                    continue
                if p.is_symlink() or not p.is_file() or n == SIDECAR:
                    continue
                out.append(p)

    @staticmethod
    def _is_manifest(name):
        n = name.lower()
        return (n.endswith("sha256sums") or n.endswith(".sha256") or n == "device-sha256.txt"
                or (n.endswith((".json", ".jsonl")) and n != SIDECAR))

    def _is_candidate(self, f):
        name = f.name
        e = ext_of(split_wrapper(name)[0]) if split_wrapper(name)[1] else ext_of(name)
        if ext_of(name) in IMAGE_EXT or e in IMAGE_EXT:
            return True
        if ext_of(name) in TEXT_EXT or ext_of(name) == ".gz":
            return False
        try:
            with open(f, "rb") as fh:
                head = fh.read(4096)
        except OSError:
            return False
        kind = sniff(head, f.stat().st_size)
        return kind is not None

    def _key(self, reldir, name):
        full = Path(reldir, name)
        base, _ = split_wrapper(full.name)
        return (full.parent.as_posix(), base)

    def _get(self, reldir, name):
        dev, ddir, entries = self._cur
        full = Path(reldir, name)
        base, codec = split_wrapper(full.name)
        key = (full.parent.as_posix(), base)
        e = entries.get(key)
        if e is None:
            e = entries[key] = {"device": dev, "dir": key[0], "name": base, "raw": new_part(),
                                "wrapped": new_part(), "recorded_in": [], "meta": {}}
        return e, codec

    def _record(self, mdir, name, sha=None, nbytes=None, by=None, device=False, meta=None):
        dev, ddir, entries = self._cur
        name = name.lstrip("*").strip()
        if not name or name.startswith("/") or ".." in Path(name).parts:
            return
        full = Path(os.path.normpath(mdir / name))
        rel = full.relative_to(ddir) if full.is_relative_to(ddir) else None
        if rel is None or is_private(rel) or ext_of(name) in TEXT_EXT:
            return
        if not ext_of(name) and (ddir / rel).is_file():
            try:
                with open(ddir / rel, "rb") as fh:
                    if looks_text(fh.read(4096)):
                        return
            except OSError:
                pass
        e, codec = self._get(rel.parent.as_posix(), rel.name)
        part = e["wrapped"] if codec else e["raw"]
        part["name"] = rel.name
        if codec:
            part["codec"] = codec
        if sha:
            sha = sha.lower()
            field = "device_sha256" if device else "sha256"
            if part[field] and part[field] != sha:
                self.conflicts.append(f"{dev}/{rel}: {by} says {sha[:12]}, earlier record {part[field][:12]}")
            else:
                part[field] = sha
        if nbytes is not None:
            part["bytes"] = int(nbytes)
        if by and by not in e["recorded_in"]:
            e["recorded_in"].append(by)
        for k, v in (meta or {}).items():
            e["meta"].setdefault(k, v)

    def _read_manifest(self, m):
        dev, ddir, _ = self._cur
        by = m.relative_to(ddir).as_posix()
        mdir = m.parent
        n = m.name.lower()
        try:
            if m.stat().st_size > (8 << 20):
                return
            text = m.read_text(errors="replace")
        except OSError as exc:
            self.warnings.append(f"{dev}/{by}: unreadable ({exc})")
            return
        if n.endswith((".json", ".jsonl")):
            rows = []
            try:
                if n.endswith(".jsonl"):
                    rows = [json.loads(line) for line in text.splitlines() if line.strip()]
                else:
                    doc = json.loads(text)
                    rows = doc if isinstance(doc, list) else doc.get("files", []) if isinstance(doc, dict) else []
            except (ValueError, AttributeError):
                return
            for r in rows:
                if not isinstance(r, dict) or not isinstance(r.get("file"), str) or not r.get("sha256"):
                    continue
                meta = {k: r[k] for k in ("source", "method", "date", "address", "bank") if k in r}
                self._record(mdir, r["file"], r["sha256"], r.get("bytes"), by, meta=meta)
                for ext, codec in WRAPPERS.items():
                    k = codec if codec != "zstd" else "zst"
                    if r.get(f"{k}_sha256"):
                        self._record(mdir, r["file"] + ext, r[f"{k}_sha256"], r.get(f"{k}_bytes"), by)
            return
        device = "device" in n
        lines = [l for l in text.splitlines() if l.strip() and not l.lstrip().startswith("#")]
        for line in lines:
            mt = re.match(r"^([0-9a-fA-F]{64})\s+(.*)$", line.strip())
            if not mt:
                continue
            sha, rest = mt.group(1), mt.group(2).strip()
            if rest in ("-", "*-"):
                # stdin hash: "<stem>[-host].sha256" names the image
                stem = re.sub(r"-(host|device)$", "", m.name[:-len(".sha256")] if n.endswith(".sha256") else "")
                target = self._match_stem(mdir, stem)
                if target:
                    self._record(mdir, target, sha, by=by)
                else:
                    self.warnings.append(f"{dev}/{by}: can't tell which file the stdin hash belongs to")
                continue
            if rest.startswith("/dev/"):
                devname = re.sub(r"(mtd\d+)ro$", r"\1", rest.split()[0][5:])
                target = self._match_stem(mdir, devname, prefix=True)
                if target:
                    self._record(mdir, target, sha, by=by, device=True, meta={"source": rest.split()[0]})
                else:
                    self.warnings.append(f"{dev}/{by}: no file for {rest.split()[0]}")
                continue
            name = rest.split(" (")[0].strip() if device else rest
            self._record(mdir, name, sha, by=by, device=device)

    def _match_stem(self, mdir, stem, prefix=False):
        if not stem:
            return None
        cands = []
        for p in sorted(mdir.iterdir()):
            if not p.is_file() or is_private(p.name):
                continue
            base, _ = split_wrapper(p.name)
            root = os.path.splitext(base)[0]
            if root == stem or base == stem or (prefix and re.match(re.escape(stem) + r"[-_.]", base)):
                cands.append(base)
        cands = sorted(set(cands))
        return cands[0] if len(cands) == 1 else None

    def _finish(self, e):
        raw, wr = e["raw"], e["wrapped"]
        stored = wr if wr["present"] else raw if raw["present"] else wr if wr["name"] else raw
        e["stored"] = stored
        sname = stored["name"] or e["name"]
        e["path"] = (Path("devices", e["device"], e["dir"], sname)).as_posix()
        e["lpath"] = (Path(e["dir"], e["name"])).as_posix()
        e["abs"] = self.root / e["path"]
        if not e["recorded_in"]:
            e["status"] = "untracked"
        elif not raw["present"] and not wr["present"]:
            e["status"] = "missing"
        elif wr["present"] and not raw["present"] and (raw["sha256"] or raw["device_sha256"] or raw["bytes"]):
            e["status"] = "raw-removed"
        else:
            e["status"] = "ok"
        sha = stored["sha256"] or (stored["device_sha256"] if stored is raw else None)
        e["sha_cached"] = False
        if not sha and stored["present"]:
            st = e["abs"].stat()
            sha = self.cache.get(e["abs"], st)
            e["sha_cached"] = bool(sha)
        e["sha256"] = sha
        e["raw_sha256"] = raw["sha256"] or raw["device_sha256"] or (sha if stored is raw else None)
        e["content_sha256"] = e["raw_sha256"]
        e["hashes"] = {h for h in (sha, raw["sha256"], raw["device_sha256"], wr["sha256"]) if h}
        recorded = stored["sha256"] or (stored["device_sha256"] if stored is raw else None)
        e["id"] = (recorded or e["raw_sha256"] or sha or "")[:12] or "-"   # recorded hashes first: stable
        e["bytes"] = raw["bytes"] or raw["size"] or (stored["size"] if stored is raw else None)
        if e["bytes"] is None and stored is wr and wr["present"] and wr.get("codec") == "zstd":
            e["bytes"] = zstd_frame_size(e["abs"])   # frame header, when the compressor knew the size
        e["stored_bytes"] = stored["size"] if stored["size"] is not None else stored["bytes"]
        e["codec"] = wr.get("codec") if stored is wr else None
        date = e["meta"].get("date")
        if date:
            e["date"], e["date_src"] = str(date), "manifest"
        elif stored["mtime"]:
            e["date"] = datetime.datetime.fromtimestamp(stored["mtime"]).astimezone().isoformat(timespec="seconds")
            e["date_src"] = "mtime"
        else:
            e["date"], e["date_src"] = None, None
        m = re.match(r"(u\d{2,})(/|$)", re.sub(r"^(dumps|dump|images|inventory)/", "", e["dir"]))
        e["unit_auto"] = m.group(1) if m else None
        e["side"] = {}
        self._apply_side(e)

    def _apply_side(self, e):
        s = e["side"]
        detect = s.get("detect") or {}
        e["tags"] = sorted(s.get("tags", []))
        e["notes"] = s.get("notes", [])
        e["detect"] = detect
        e["unit"] = s.get("unit") or e["unit_auto"]
        e["kind"] = s.get("kind") or classify(e["name"], e["dir"], detect)
        e["version"] = s.get("version") or detect.get("version") or version_from_name(e["name"])

    # -- lookup
    def resolve(self, ref):
        ref = ref.strip()
        cands = []
        if ":" in ref and not ref.startswith("/"):
            dev, rel = ref.split(":", 1)
            rel = rel.strip("/")
            cands = [e for e in self.entries if e["device"] == dev and
                     rel in (e["lpath"], str(Path(e["path"]).relative_to(Path("devices", dev))))]
        else:
            paths = set()
            for base in (Path.cwd(), self.root, self.root / "devices"):
                p = (base / ref)
                paths.add(os.path.normpath(str(p if p.is_absolute() else p.absolute())))
            for e in self.entries:
                mine = {os.path.normpath(str(self.root / "devices" / e["device"] / e["dir"] / n))
                        for n in (e["raw"]["name"], e["wrapped"]["name"], e["name"]) if n}
                if mine & paths:
                    cands.append(e)
            if not cands and re.fullmatch(r"[0-9a-fA-F]{6,64}", ref):
                r = ref.lower()
                cands = [e for e in self.entries if any(h.startswith(r) for h in e["hashes"])]
        if not cands:
            raise Fail(f"no image matches {ref!r} (try `ai-re-images list`)")
        if len(cands) > 1:
            listing = "\n  ".join(e["path"] for e in cands)
            raise Fail(f"{ref!r} matches {len(cands)} images; use a path:\n  {listing}")
        return cands[0]

    def select(self, a, refs=None):
        if refs:
            return [self.resolve(r) for r in refs]
        out = []
        for e in self.entries:
            if a.unit and e["unit"] != a.unit:
                continue
            if a.kind and e["kind"] != a.kind:
                continue
            if a.tag and not set(a.tag) <= set(e["tags"]):
                continue
            if a.status and e["status"] != a.status:
                continue
            if a.text:
                hay = " ".join([e["path"], " ".join(e["tags"]), e["version"] or "", e["kind"],
                                json.dumps(e["notes"]), (e["detect"] or {}).get("summary", "")]).lower()
                if a.text.lower() not in hay:
                    continue
            out.append(e)
        return out

    # -- sidecar writes
    def update_side(self, e, fn):
        ddir = self.root / "devices" / e["device"]
        rows = load_sidecar(ddir / SIDECAR)
        key = e.get("moved_from") or e["lpath"]
        row = next((r for r in rows if r.get("path") == key), None)
        if row is None:
            row = {"path": e["lpath"]}
            rows.append(row)
        row["path"] = e["lpath"]
        content = e["raw_sha256"] or e["sha256"]
        if content:
            row["sha256"] = content
        fn(row)
        for k in [k for k, v in row.items() if v in (None, "", [], {})]:
            del row[k]
        rows = [r for r in rows if set(r) - {"path", "sha256"}]
        write_sidecar(ddir / SIDECAR, rows)
        e["side"] = row if row in rows else {}
        e.pop("moved_from", None)
        self._apply_side(e)


def looks_rootfs(d):
    try:
        names = {p.name for p in d.iterdir() if p.is_dir()}
    except OSError:
        return False
    # an extracted filesystem tree, not a directory of images
    return "lost+found" in names or ("etc" in names and bool(names & {"bin", "usr", "var", "home", "root", "lib"}))


def load_sidecar(path):
    rows = []
    try:
        text = path.read_text()
    except OSError:
        return rows
    for n, line in enumerate(text.splitlines(), 1):
        if line.strip():
            try:
                rows.append(json.loads(line))
            except ValueError:
                raise Fail(f"{path}:{n}: bad JSON line; fix it by hand", 1)
    return rows


def write_sidecar(path, rows):
    rows = sorted(rows, key=lambda r: r.get("path", ""))
    if not rows:
        if path.exists():
            path.unlink()
        return
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text("".join(json.dumps(r, sort_keys=True) + "\n" for r in rows))
    os.replace(tmp, path)


# ---------------------------------------------------------------- classification

def version_from_name(name):
    m = VERSION_RE.search(name)
    return m.group(1) if m else None


def classify(name, reldir, detect):
    n = split_wrapper(name)[0].lower()
    d = reldir.lower()
    if "eeprom" in n or n.startswith("spd"):
        k = "eeprom"
    elif re.search(r"(^|[-_.])(u-?boot-?)?env([-_.]|$)", n):
        k = "env"
    elif re.match(r"^(sd[a-z]|mmcblk\d+|nvme\d+n\d+|hd[a-z]|ssd|disk|emmc)([-_.]|$)", n) and ".img" in n:
        k = "disk"
    elif re.match(r"^p\d+\.img$", n) or re.search(r"[-_.]part\d+", n):
        k = "partition"
    elif n.endswith(".dtb"):
        k = "dtb"
    elif n.endswith((".itb", ".fit")):
        k = "fit"
    elif re.search(r"initramfs|initrd|rootfs\.cpio|\.cpio", n):
        k = "initramfs"
    elif re.search(r"kernel|vmlinu|zimage|^image$", n):
        k = "kernel"
    elif "rootfs" in n:
        k = "rootfs"
    elif "bootrom" in n:
        k = "rom"
    elif "config" in n:
        k = "config"
    elif (re.match(r"^(mtd\d+|nor|spi|flash\d*)([-_.]|$)", n) or re.search(r"(^|/)spi[-_]", d)
          or re.search(r"(p25q|w25q|mx25|_good\.bin$)", n)):
        k = "flash"
    elif n.endswith(".dav") or "firmware" in d or "uimage" in n:
        k = "firmware"
    else:
        k = "other"
    dk = (detect or {}).get("kind")
    if dk and (k in ("other", "firmware") or (k == "partition" and dk == "disk")
               or (k == "flash" and dk in ("env", "eeprom"))):
        return dk
    return k


# ---------------------------------------------------------------- detection (read-only, from bytes)

UIMG_OS = {5: "linux", 17: "u-boot", 0: "invalid", 1: "openbsd", 2: "netbsd", 3: "freebsd"}
UIMG_ARCH = {2: "arm", 3: "x86", 5: "mips", 6: "mips64", 7: "ppc", 22: "arm64", 24: "x86_64", 26: "riscv"}
UIMG_TYPE = {1: "standalone", 2: "kernel", 3: "ramdisk", 4: "multi", 5: "firmware", 6: "script",
             7: "filesystem", 8: "flat_dt", 14: "kernel_noload"}
UIMG_COMP = {0: "none", 1: "gzip", 2: "bzip2", 3: "lzma", 4: "lzo", 5: "lz4", 6: "zstd"}
TLV_SAFE = {0x21: "product_name", 0x22: "part_number", 0x25: "manufacture_date", 0x26: "device_version",
            0x27: "label_revision", 0x28: "platform_name", 0x29: "onie_version", 0x2A: "mac_count",
            0x2B: "manufacturer", 0x2C: "country", 0x2D: "vendor", 0x2E: "diag_version"}
TLV_WITHHELD = {0x23: "serial_number", 0x24: "base_mac", 0x2F: "service_tag", 0xFD: "vendor_extension"}
GPT_TYPES = {"0FC63DAF-8483-4772-8E79-3D69D8477DE4": "linux", "C12A7328-F81F-11D2-BA4B-00A0C93EC93B": "efi",
             "0657FD6D-A4AB-43C4-84E5-0933C84B4F4F": "swap", "E6D6D379-F507-44C2-A23C-238F2A3DF928": "lvm",
             "21686148-6449-6E6F-744E-656564454649": "bios-boot", "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7": "msdata",
             "A19D880F-05FC-4D3B-A006-743F0F84911E": "raid"}
MBR_TYPES = {"83": "linux", "82": "swap", "5": "extended", "f": "extended", "85": "extended", "b": "fat32",
             "c": "fat32", "e": "fat16", "6": "fat16", "ee": "gpt-protective", "8e": "lvm", "ef": "efi", "fd": "raid"}
EXTENDED = "extended"


def u32le(b, o):
    return struct.unpack_from("<I", b, o)[0] if len(b) >= o + 4 else None


def uimage_header(b, off=0):
    if len(b) < off + 64 or b[off:off + 4] != b"\x27\x05\x19\x56":
        return None
    hdr = bytes(b[off:off + 64])
    magic, hcrc, tm, size, load, ep, dcrc, os_, arch, typ, comp = struct.unpack(">7I4B", hdr[:32])
    if binascii.crc32(hdr[:4] + b"\0\0\0\0" + hdr[8:]) != hcrc:
        return None
    name = hdr[32:64].split(b"\0")[0].decode(errors="replace").strip()
    return {"name": name, "type": UIMG_TYPE.get(typ, str(typ)), "os": UIMG_OS.get(os_, str(os_)),
            "arch": UIMG_ARCH.get(arch, str(arch)), "comp": UIMG_COMP.get(comp, str(comp)),
            "data_bytes": size, "load": f"0x{load:08x}", "entry": f"0x{ep:08x}",
            "created": datetime.datetime.fromtimestamp(tm, datetime.timezone.utc).strftime("%Y-%m-%d %H:%M:%SZ")}


def fdt_info(b, off=0):
    if len(b) < off + 40 or b[off:off + 4] != b"\xd0\x0d\xfe\xed":
        return None
    (_, total, o_struct, o_str, _, ver, _, _, sz_str, sz_struct) = struct.unpack_from(">10I", b, off)
    if not (16 <= ver <= 17) or total < 40 or o_struct >= total or o_str >= total:
        return None
    info = {"bytes": total, "version": ver}
    strings = bytes(b[off + o_str: off + o_str + sz_str]) if len(b) >= off + o_str else b""

    def sname(o):
        if o < len(strings):
            return strings[o:strings.index(b"\0", o) if b"\0" in strings[o:] else len(strings)].decode(errors="replace")
        return None
    pos, depth, children = off + o_struct, 0, []
    end = min(len(b), off + o_struct + (sz_struct or total))
    while pos + 4 <= end:
        tok = struct.unpack_from(">I", b, pos)[0]
        pos += 4
        if tok == 1:
            nul = bytes(b[pos:pos + 256]).find(b"\0")
            if nul < 0:
                break
            name = bytes(b[pos:pos + nul]).decode(errors="replace")
            pos += (nul + 4) & ~3
            depth += 1
            if depth == 2 and len(children) < 32:
                children.append(name)
        elif tok == 2:
            depth -= 1
            if depth <= 0:
                break
        elif tok == 3:
            if pos + 8 > end:
                break
            ln, no = struct.unpack_from(">II", b, pos)
            pos += 8
            if depth == 1:
                key = sname(no)
                if key in ("model", "compatible", "description") and pos + ln <= len(b):
                    vals = [v.decode(errors="replace") for v in bytes(b[pos:pos + ln]).split(b"\0") if v]
                    info[key] = vals if key == "compatible" else (vals[0] if vals else "")
            pos += (ln + 3) & ~3
        elif tok == 4:
            continue
        else:
            break
    info["fit"] = "images" in children
    if children:
        info["nodes"] = children[:16]
    return info


def onie_tlv(b):
    if not b.startswith(b"TlvInfo\0") or len(b) < 11:
        return None
    total = struct.unpack_from(">H", b, 9)[0]
    out, withheld, pos, end = {}, [], 11, min(len(b), 11 + total)
    crc_ok = None
    while pos + 2 <= end:
        t, ln = b[pos], b[pos + 1]
        val = bytes(b[pos + 2:pos + 2 + ln])
        if t == 0xFE and ln == 4:
            crc_ok = binascii.crc32(bytes(b[:pos + 2])) == struct.unpack(">I", val)[0]
        elif t in TLV_SAFE:
            out[TLV_SAFE[t]] = int.from_bytes(val, "big") if t in (0x26, 0x2A) else val.decode(errors="replace")
        elif t in TLV_WITHHELD:
            withheld.append(TLV_WITHHELD[t])
        else:
            withheld.append(f"0x{t:02x}")
        pos += 2 + ln
    return {"fields": out, "withheld": sorted(set(withheld)), "crc_ok": crc_ok}


def uboot_env(b, off=0, sizes=None):
    """CRC-valid U-Boot environment at OFF. Returns size/redundancy/var count only (values can be private)."""
    avail = len(b) - off
    sizes = sizes or sorted({s for s in (avail, 0x1000, 0x2000, 0x4000, 0x8000, 0x10000, 0x20000, 0x40000) if 8 < s <= avail}, reverse=True)
    for redundant in (False, True):
        d0 = off + (5 if redundant else 4)
        if not re.match(rb"[A-Za-z0-9_][A-Za-z0-9_.#-]*=", bytes(b[d0:d0 + 64])):
            continue
        if len(b) < off + 4:
            return None
        wants = struct.unpack_from("<I", b, off) + struct.unpack_from(">I", b, off)   # crc in CPU byte order
        for s in sizes:
            data = memoryview(b)[d0:off + s]
            if binascii.crc32(data) in wants:
                body = bytes(data).split(b"\0\0")[0]
                return {"offset": off, "size": s, "redundant": redundant, "vars": body.count(b"\0") + 1}
    return None


def fs_magic(b):
    if len(b) < 512:
        return None
    if b.count(0) == len(b):
        return {"fs": "zeros"}
    if len(b) >= 0x480 and b[0x438:0x43a] == b"\x53\xef":
        compat, incompat = u32le(b, 0x45C), u32le(b, 0x460)
        fs = "ext4" if incompat & (0x40 | 0x80 | 0x200) else "ext3" if compat & 0x4 else "ext2"
        r = {"fs": fs, "block": 1024 << u32le(b, 0x418), "blocks": u32le(b, 0x404)}
        label = b[0x478:0x488].split(b"\0")[0].decode(errors="replace")
        if label:
            r["label"] = label
        return r
    if b[:4] in (b"hsqs", b"sqsh"):
        comp = {1: "gzip", 2: "lzma", 3: "lzo", 4: "xz", 5: "lz4", 6: "zstd"}.get(struct.unpack_from("<H", b, 20)[0])
        return {"fs": "squashfs", "comp": comp, "bytes": struct.unpack_from("<Q", b, 40)[0]}
    if b[510:512] == b"\x55\xaa" and (b[0x36:0x3b] in (b"FAT12", b"FAT16") or b[0x52:0x57] == b"FAT32"):
        lab = (b[0x47:0x52] if b[0x52:0x57] == b"FAT32" else b[0x2b:0x36]).decode(errors="replace").strip()
        return {"fs": "vfat", **({"label": lab} if lab and lab != "NO NAME" else {})}
    checks = [(3, b"NTFS    ", "ntfs"), (0, b"XFSB", "xfs"), (0, b"LUKS\xba\xbe", "luks"), (0, b"UBI#", "ubi"),
              (512, b"LABELONE", "lvm2"), (4086, b"SWAPSPACE2", "swap"), (4086, b"SWAP-SPACE", "swap"),
              (0x10040, b"_BHRfS_M", "btrfs"), (0x8001, b"CD001", "iso9660"), (0, b"-rom1fs-", "romfs"),
              (0, b"\x45\x3d\xcd\x28", "cramfs"), (0x400, b"\x10\x20\xf5\xf2", "f2fs"), (0x400, b"\xe2\xe1\xf5\xe0", "erofs")]
    for o, magic, fs in checks:
        if b[o:o + len(magic)] == magic:
            return {"fs": fs}
    if b[:2] in (b"\x85\x19", b"\x19\x85"):
        return {"fs": "jffs2"}
    return None


def mbr_plausible(b):
    seen = 0
    for i in range(4):
        st, typ = b[446 + 16 * i], b[446 + 16 * i + 4]
        start, count = struct.unpack_from("<II", b, 446 + 16 * i + 8)
        if st not in (0, 0x80):
            return False
        if typ:
            if not start or not count:
                return False
            seen += 1
    return seen > 0


def sniff(b, size=None):
    """Cheap content type at offset 0 (used for scan candidates and inspect)."""
    if uimage_header(b):
        return "uimage"
    f = fdt_info(b)
    if f:
        return "fit" if f["fit"] else "fdt"
    if b.startswith(b"TlvInfo\0"):
        return "onie-tlv"
    if b[:4] == b"\x28\xb5\x2f\xfd":
        return "zstd"
    if b[:6] == b"\xfd7zXZ\0":
        return "xz"
    if b[:6] in (b"070701", b"070702"):
        return "cpio"
    if b[:4] in (b"hsqs", b"sqsh", b"UBI#"):
        return "fs"
    if b[:8] == b"ANDROID!":
        return "android-boot"
    if len(b) >= 520 and b[510:512] == b"\x55\xaa" and (b[512:520] == b"EFI PART" or mbr_plausible(b)):
        return "mbr"
    return None


def scan_embedded(chunks, small):
    """Magic map over the whole content: uImage/FDT anywhere (validated), others 4 KiB-aligned."""
    hits, counts, tail, base = [], {}, b"", 0
    total = ff = zero = 0
    env_hits = []
    seen = set()
    for chunk in chunks:
        total += len(chunk)
        ff += chunk.count(b"\xff")
        zero += chunk.count(b"\0")
        buf = tail + chunk
        boff = base - len(tail)
        for magic, fn, kind in ((b"\x27\x05\x19\x56", uimage_header, "uimage"), (b"\xd0\x0d\xfe\xed", fdt_info, "fdt")):
            i = buf.find(magic)
            while i >= 0:
                o = boff + i
                if o not in seen and o != 0:
                    info = fn(buf, i)
                    if info:
                        seen.add(o)
                        hits.append({"off": f"0x{o:x}", "type": kind, **_brief(kind, info)})
                i = buf.find(magic, i + 1)
        start = (-boff) % 4096
        for i in range(start, len(buf) - 8, 4096):
            o = boff + i
            if o == 0 or o in seen:
                continue
            w = buf[i:i + 8]
            t = ("squashfs" if w[:4] in (b"hsqs", b"sqsh") else "ubi" if w[:4] == b"UBI#" else
                 "jffs2" if w[:2] in (b"\x85\x19", b"\x19\x85") and w[2:4] in (b"\x01\xe0", b"\xe0\x01", b"\x02\xe0", b"\xe0\x02", b"\x03\x20", b"\x20\x03", b"\x06\x20", b"\x20\x06") else
                 "gzip" if w[:3] == b"\x1f\x8b\x08" else "xz" if w[:6] == b"\xfd7zXZ\0" else
                 "zstd" if w[:4] == b"\x28\xb5\x2f\xfd" else "elf" if w[:4] == b"\x7fELF" else
                 "cpio" if w[:6] in (b"070701", b"070702") else "onie-tlv" if w == b"TlvInfo\0" else None)
            if t:
                seen.add(o)
                c = counts.setdefault(t, {"type": t, "count": 0, "first": f"0x{o:x}"})
                c["count"] += 1
                c["last"] = f"0x{o:x}"
            if small and i % 0x10000 == (-boff) % 0x10000 and len(env_hits) < 8:
                env = uboot_env(buf, i, sizes=[s for s in (0x1000, 0x2000, 0x4000, 0x8000, 0x10000, 0x20000) if i + s <= len(buf)])
                if env and o != 0:
                    seen.add(o)
                    env_hits.append({"off": f"0x{o:x}", "type": "uboot-env", "size": env["size"], "redundant": env["redundant"], "vars": env["vars"]})
        tail = buf[-(256 << 10):] if small else buf[-128:]
        base += len(chunk)
    emb = hits[:24] + env_hits + sorted(counts.values(), key=lambda c: int(c["first"], 16))
    fill = None
    if total:
        fill = "erased (all 0xff)" if ff == total else "all zero" if zero == total else f"{100 * ff / total:.0f}% 0xff, {100 * zero / total:.0f}% 0x00"
    return emb, fill


def embedded_summary(emb):
    groups = {}
    for x in emb:
        g = groups.setdefault(x["type"], {"n": 0, "names": []})
        g["n"] += x.get("count", 1)
        label = x.get("name") or x.get("model") or (f"@{x['off']}" if x["type"] == "uboot-env" else None)
        if label and label not in g["names"]:
            g["names"].append(label)
    parts = []
    for t, g in groups.items():
        names = f" ({', '.join(repr(n) if not n.startswith('@') else n for n in g['names'][:4])}{', …' if len(g['names']) > 4 else ''})" if g["names"] else ""
        parts.append(f"{t}×{g['n']}{names}")
    return ", ".join(parts)


def _brief(kind, info):
    if kind == "uimage":
        return {"name": info["name"], "img": info["type"], "comp": info["comp"]}
    keep = {k: info[k] for k in ("model", "description") if k in info}
    return {"bytes": info["bytes"], "fit": info["fit"], **keep}


def scrub(text):
    return re.sub(r"UUID=[0-9a-fA-F-]+", "UUID=<scrubbed>", text)


def zstd_frame_size(path):
    with open(path, "rb") as f:
        h = f.read(18)
    if len(h) < 6 or h[:4] != b"\x28\xb5\x2f\xfd":
        return None
    fhd = h[4]
    fcs, single, dictf = fhd >> 6, (fhd >> 5) & 1, fhd & 3
    pos = 5 + (0 if single else 1) + (0, 1, 2, 4)[dictf]
    ln = (1 if single else 0, 2, 4, 8)[fcs]
    if not ln or len(h) < pos + ln:
        return None
    v = int.from_bytes(h[pos:pos + ln], "little")
    return v + 256 if ln == 2 else v


def xz_size(path):
    try:
        out = subprocess.run(["xz", "--robot", "-l", str(path)], capture_output=True, text=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    for line in out.splitlines():
        f = line.split("\t")
        if f[0] == "totals" and len(f) > 4:
            return int(f[4])
    return None


def run_tool(cmd, **kw):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=120, **kw)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return subprocess.CompletedProcess(cmd, 127, "", str(exc))


class Content:
    """Read-only view of an entry's decompressed bytes."""

    def __init__(self, path, codec, raw_size):
        self.path, self.codec, self.raw_size = path, codec, raw_size

    def head(self, n=HEAD):
        if not self.codec:
            with open(self.path, "rb") as f:
                return f.read(n)
        buf = bytearray()
        with stream(self.path, self.codec) as p:
            while len(buf) < n and (c := p.stdout.read(min(CHUNK, n - len(buf)))):
                buf += c
        return bytes(buf)

    def chunks(self):
        if not self.codec:
            with open(self.path, "rb") as f:
                while c := f.read(CHUNK):
                    yield c
            return
        with stream(self.path, self.codec) as p:
            while c := p.stdout.read(CHUNK):
                yield c

    def ranges(self, want, limit):
        """{offset: bytes} for (offset, length) pairs; a stream is read once, up to LIMIT."""
        out = {o: bytearray() for o, _ in want}
        if not self.codec:
            with open(self.path, "rb") as f:
                for o, ln in want:
                    f.seek(o)
                    out[o] += f.read(ln)
            return out, []
        limit = float("inf") if limit is None else limit
        need = sorted(w for w in want if w[0] < limit)
        skipped = [o for o, _ in want if o >= limit]
        pos = 0
        with stream(self.path, self.codec) as p:
            while need:
                c = p.stdout.read(CHUNK)
                if not c:
                    break
                end = pos + len(c)
                for o, ln in need:
                    if o < end and o + ln > pos:
                        out[o] += c[max(o, pos) - pos:min(o + ln, end) - pos]
                need = [(o, ln) for o, ln in need if o + ln > end]
                pos = end
        return out, skipped

    @contextlib.contextmanager
    def materialized(self, scratch):
        if not self.codec:
            yield self.path
            return
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch) as td:
            p = Path(td) / "content"
            with open(p, "wb") as f:
                for c in self.chunks():
                    f.write(c)
            yield p


def detect(e, root, deep=False, scratch=None):
    stored = e["stored"]
    if not stored["present"]:
        raise Fail(f"{e['path']}: file not present", 1)
    path, codec = e["abs"], e["codec"]
    scratch = scratch or cache_dir() / "tmp"
    res = {"at": today()}
    raw_size = e["raw"]["bytes"] or (stored["size"] if not codec else None)
    if codec:
        w = {"codec": codec}
        frame = zstd_frame_size(path) if codec == "zstd" else xz_size(path)
        if frame:
            w["raw_bytes"] = frame
            raw_size = raw_size or frame
        res["wrapper"] = w
    c = Content(path, codec, raw_size)
    head = c.head()
    if raw_size is None and len(head) < HEAD:
        raw_size = len(head)
    parts = []
    kind = sniff(head, raw_size)
    summary = []
    if head[512:520] == b"EFI PART" or (kind == "mbr" and not fs_magic(head)):
        res["kind"] = "disk"
        table = partition_table(c, head, raw_size, scratch)
        res.update(table)
        want = [(p["start"] * 512, 128 << 10) for p in table.get("partitions", []) if p.get("type") != EXTENDED]
        got, skipped = c.ranges(want, None if deep or not codec else STREAM_LIMIT) if want else ({}, [])
        for p in table.get("partitions", []):
            o = p["start"] * 512
            if o in skipped:
                p["fs"] = "not read (beyond stream limit; --deep)"
            elif o in got:
                fsm = fs_magic(bytes(got[o]))
                if fsm:
                    p.update({k: v for k, v in fsm.items() if k in ("fs", "label", "comp")})
            parts.append(f"p{p['n']} {p.get('fs', p.get('type'))}"
                         + (f" '{p['label']}'" if p.get("label") else "") + f" {human(p['size'] * 512)}")
        summary.append(f"{table.get('label', '?').upper()}: " + ", ".join(parts))
    elif (fsm := fs_magic(head)) and fsm["fs"] != "zeros":
        res["kind"], res["fs"] = "partition", fsm
        summary.append(fsm["fs"] + (f" '{fsm['label']}'" if fsm.get("label") else ""))
    elif kind == "uimage":
        u = uimage_header(head)
        res["uimage"] = u
        res["kind"] = {"kernel": "kernel", "kernel_noload": "kernel", "ramdisk": "initramfs", "flat_dt": "dtb"}.get(u["type"], "firmware")
        summary.append(f"uImage {u['type']} {u['arch']} {u['comp']} '{u['name']}' {u['created'][:10]}")
        v = version_from_name(u["name"])
        if v:
            res["version"] = v
    elif kind in ("fit", "fdt"):
        f = fdt_info(head)
        if f and "model" not in f and not codec and f["bytes"] > len(head):
            with open(path, "rb") as fh:
                f = fdt_info(fh.read(min(f["bytes"], 64 << 20))) or f
        res["fdt"] = f
        res["kind"] = "fit" if f["fit"] else "dtb"
        summary.append(("FIT " + repr(f.get("description", ""))) if f["fit"] else f"DTB model {f.get('model', '?')!r}")
    elif kind == "onie-tlv":
        t = onie_tlv(head)
        res["kind"], res["onie_tlv"] = "eeprom", t
        summary.append("ONIE TlvInfo " + " ".join(str(t["fields"].get(k, "")) for k in ("product_name", "part_number")).strip()
                       + (" crc ok" if t["crc_ok"] else " crc BAD" if t["crc_ok"] is False else ""))
    elif (env := uboot_env(head)) and (raw_size or 0) <= (4 << 20):
        res["kind"], res["uboot_env"] = "env", env
        summary.append(f"U-Boot env {env['size']:#x}{' redundant' if env['redundant'] else ''}, {env['vars']} vars")
    elif head[:4] == b"\x7fELF":
        cls = {1: "elf32", 2: "elf64"}.get(head[4], "elf")
        endian = "<" if head[5] == 1 else ">"
        mach = struct.unpack_from(endian + "H", head, 18)[0]
        res["elf"] = {"class": cls, "machine": {0x3e: "x86_64", 0xb7: "aarch64", 0x28: "arm", 0x14: "ppc", 0x15: "ppc64", 0x08: "mips", 0xf3: "riscv"}.get(mach, mach)}
        summary.append(f"ELF {cls} {res['elf']['machine']}")
    elif kind == "cpio":
        res["kind"] = "initramfs"
        summary.append("cpio newc")
    elif head[:2] == b"\x1f\x8b":
        summary.append("gzip data")
    elif kind in ("zstd", "xz"):
        summary.append(f"{kind} data (inner)")
    if kind in ("uimage", "fit") and (raw_size or 0) <= (512 << 20) and shutil.which("dumpimage"):
        with c.materialized(scratch) as p:
            r = run_tool(["dumpimage", "-l", str(p)])
        lines = [l.rstrip() for l in (r.stdout + r.stderr).splitlines() if l.strip()]
        res["dumpimage"] = [scrub(l) for l in lines[:40]]
    if raw_size is not None and raw_size <= EMBED_MAX:
        emb, fill = scan_embedded(c.chunks(), small=(raw_size or 0) <= (64 << 20))
        if emb:
            res["embedded"] = emb
        if fill:
            res["fill"] = fill
        if not summary and emb:
            summary.append("contains " + embedded_summary(emb))
    fill = res.get("fill")
    if fill and fill.startswith(("erased", "all zero")):
        summary.append(fill)
    if not summary and shutil.which("file"):
        r = subprocess.run(["file", "-b", "-"], input=head[:65536], capture_output=True)
        res["file"] = scrub(r.stdout.decode(errors="replace").strip())[:200]
        summary.append(res["file"])
        if fill and res["file"] == "data":
            summary.append(fill)
    res["summary"] = "; ".join(s for s in summary if s)[:300]
    return res


def partition_table(c, head, raw_size, scratch):
    size = raw_size
    if head[512:520] == b"EFI PART" and len(head) >= 600:
        alt = struct.unpack_from("<Q", head, 512 + 32)[0]
        size = size or (alt + 1) * 512
    out = {}
    with contextlib.ExitStack() as stack:
        if c.codec:
            # sfdisk wants the real device size: a sparse file holding only the head
            scratch.mkdir(parents=True, exist_ok=True)
            td = stack.enter_context(tempfile.TemporaryDirectory(dir=scratch))
            target = Path(td) / "head.img"
            with open(target, "wb") as f:
                f.write(head)
                if size and size > len(head):
                    f.truncate(size)
        else:
            target = c.path
        r = run_tool(["sfdisk", "--json", str(target)])
    try:
        t = json.loads(r.stdout)["partitiontable"]
    except (ValueError, KeyError):
        return {"label": "unknown", "partitions": [], "error": scrub(r.stderr.strip())[:200]}
    out["label"] = t.get("label")
    out["partitions"] = []
    for n, p in enumerate(t.get("partitions", []), 1):
        num = re.fullmatch(r"p?(\d+)", p.get("node", "")[len(str(target)):])
        typ = str(p.get("type", ""))
        q = {"n": int(num.group(1)) if num else n, "start": p["start"], "size": p["size"],
             "type": GPT_TYPES.get(typ.upper(), MBR_TYPES.get(typ.lower(), typ))}
        if p.get("name"):
            q["name"] = p["name"]
        if p.get("bootable"):
            q["bootable"] = True
        out["partitions"].append(q)
    warn = [l for l in r.stderr.splitlines() if l.strip() and "backup GPT" not in l and "PMBR size" not in l]
    if c.codec and any(p["type"] == EXTENDED for p in out["partitions"]):
        warn.append("logical partitions inside the extended one are not read from a compressed stream (export-qcow2 first)")
    if warn:
        out["warnings"] = [scrub(w)[:200] for w in warn[:4]]
    return out


# ---------------------------------------------------------------- output

def public(e):
    keys = ["id", "device", "unit", "kind", "version", "status", "path", "bytes", "stored_bytes", "codec",
            "sha256", "raw_sha256", "date", "date_src", "tags", "notes", "recorded_in"]
    d = {k: e.get(k) for k in keys}
    d["device_sha256"] = e["raw"]["device_sha256"]
    d["meta"] = e["meta"]
    d["detect"] = e["detect"] or None
    d["sha_cached"] = e["sha_cached"]
    return d


def table(rows, cols):
    if not rows:
        return
    widths = [max(len(c), *(len(str(r[i])) for r in rows)) for i, c in enumerate(cols)]
    print("  ".join(c.ljust(w) for c, w in zip(cols, widths)).rstrip())
    for r in rows:
        print("  ".join(str(v).ljust(w) for v, w in zip(r, widths)).rstrip())


def clip(s, n):
    return s if len(s) <= n else s[:n - 1] + "…"


def emit(a, obj, text_fn):
    if a.json:
        print(json.dumps(obj, indent=1, sort_keys=True, default=str))
    else:
        text_fn()


def short_status(e):
    return {"raw-removed": "zst-only" if e["codec"] == "zstd" else "packed-only"}.get(e["status"], e["status"])


# ---------------------------------------------------------------- commands

def cmd_scan(a, cat):
    if a.hash:
        for e in cat.entries:
            st = e["stored"]
            if st["present"] and not st["sha256"] and not (st is e["raw"] and e["raw"]["device_sha256"]):
                s = e["abs"].stat()
                if not cat.cache.get(e["abs"], s):
                    sha, _ = hash_file(e["abs"], progress_printer(e["path"], s.st_size))
                    cat.cache.put(e["abs"], s, sha)
        cat.cache.save()
        cat = Catalog(cat.root, a.device and [a.device], cat.cache)
    per = {}
    for e in cat.entries:
        d = per.setdefault(e["device"], {"images": 0, "ok": 0, "raw-removed": 0, "missing": 0, "untracked": 0})
        d["images"] += 1
        d[e["status"]] += 1
    untracked = [e for e in cat.entries if e["status"] == "untracked"]
    missing = [e for e in cat.entries if e["status"] == "missing"]
    obj = {"devices": per, "untracked": [{"path": e["path"], "bytes": e["stored_bytes"], "sha256": e["sha256"]} for e in untracked],
           "missing": [e["path"] for e in missing], "conflicts": cat.conflicts, "orphan_sidecar_rows": cat.orphans,
           "warnings": cat.warnings, "private_files_skipped": cat.private_skipped}

    def text():
        table([[d, v["images"], v["ok"], v["raw-removed"], v["missing"], v["untracked"]] for d, v in per.items()],
              ["DEVICE", "IMAGES", "OK", "ZST-ONLY", "MISSING", "UNTRACKED"])
        if untracked:
            print(f"\nuntracked (no manifest lists them){'' if a.hash else '; --hash to give them an identity'}:")
            for e in untracked:
                print(f"  {e['id']:12}  {human(e['stored_bytes']):>6}  {e['path']}")
        if missing:
            print("\nmissing (recorded, file not here):")
            for e in missing:
                print(f"  {e['path']}")
        for title, items in (("conflicts", cat.conflicts), ("sidecar rows with no image", cat.orphans), ("warnings", cat.warnings)):
            if items:
                print(f"\n{title}:")
                for i in items:
                    print(f"  {i}")
        if cat.private_skipped:
            print(f"\n{cat.private_skipped} private file(s) skipped")
    emit(a, obj, text)
    return 1 if cat.conflicts or cat.orphans else 0


SORTS = {"date": lambda e: e["date"] or "", "size": lambda e: e["bytes"] or e["stored_bytes"] or 0,
         "device": lambda e: (e["device"], e["unit"] or "", e["lpath"]),
         "version": lambda e: [int(x) for x in re.findall(r"\d+", e["version"] or "")] or [-1],
         "kind": lambda e: (e["kind"], e["device"], e["lpath"]), "unit": lambda e: (e["unit"] or "", e["device"], e["lpath"]),
         "path": lambda e: e["path"]}


def cmd_list(a, cat):
    rows = sorted(cat.select(a), key=SORTS[a.sort], reverse=a.reverse)

    def text():
        table([[e["id"], e["device"], e["unit"] or "-", e["kind"], human(e["bytes"] or e["stored_bytes"]),
                short_status(e), (e["date"] or "-")[:10], e["version"] or "-", clip(",".join(e["tags"]) or "-", 44),
                str(Path(e["path"]).relative_to(Path("devices", e["device"])))] for e in rows],
              ["ID", "DEVICE", "UNIT", "KIND", "SIZE", "STATUS", "DATE", "VERSION", "TAGS", "PATH"])
        print(f"{len(rows)} image(s)", file=sys.stderr)
    emit(a, [public(e) for e in rows], text)
    return 0


def cmd_show(a, cat):
    e = cat.resolve(a.ref)
    d = public(e)

    def text():
        for k in ("id", "device", "unit", "kind", "version", "status", "path", "codec", "bytes", "stored_bytes",
                  "sha256", "raw_sha256", "device_sha256", "date", "recorded_in", "tags"):
            v = d[k]
            if v in (None, [], ""):
                continue
            if k in ("bytes", "stored_bytes"):
                v = f"{v} ({human(v)})"
            if k == "sha256" and d["sha_cached"]:
                v = f"{v} (computed, not recorded)"
            if k == "date":
                v = f"{v} ({d['date_src']})"
            if isinstance(v, list):
                v = ", ".join(v)
            print(f"{k:14} {v}")
        for k, v in d["meta"].items():
            if k != "date":
                print(f"{k:14} {v}")
        for n in d["notes"]:
            print(f"{'note':14} {n.get('date', '')} {n.get('text', '')}")
        if d["detect"]:
            print(f"{'detected':14} {d['detect'].get('summary', '')}  (inspect {d['detect'].get('at', '')})")
            for p in d["detect"].get("partitions", []):
                print(f"{'':14} p{p['n']} start {p['start']} size {human(p['size'] * 512)} {p['type']} {p.get('fs', '')} {p.get('label', '')}".rstrip())
            for x in d["detect"].get("embedded", []):
                print(f"{'':14} {json.dumps(x, sort_keys=True)}")
    emit(a, d, text)
    return 0


def check_tags(tags):
    bad = [t for t in tags if not TAG_RE.match(t)]
    if bad:
        raise Fail(f"bad tag(s) {bad}: use lowercase letters, digits, . _ + -")


def cmd_tag(a, cat):
    e = cat.resolve(a.ref)
    check_tags(a.tags)
    if a.cmd == "tag":
        cat.update_side(e, lambda r: r.__setitem__("tags", sorted(set(r.get("tags", [])) | set(a.tags))))
    else:
        cat.update_side(e, lambda r: r.__setitem__("tags", sorted(set(r.get("tags", [])) - set(a.tags))))
    emit(a, public(e), lambda: print(f"{e['path']}: {', '.join(e['tags']) or '(no tags)'}"))
    return 0


def cmd_note(a, cat):
    e = cat.resolve(a.ref)
    text = " ".join(a.text).strip()
    if not text:
        raise Fail("empty note")
    cat.update_side(e, lambda r: r.setdefault("notes", []).append({"date": today(), "text": text}))
    emit(a, public(e), lambda: print(f"{e['path']}: {len(e['notes'])} note(s)"))
    return 0


def cmd_set(a, cat):
    e = cat.resolve(a.ref)
    changes = {k: getattr(a, k) for k in ("kind", "unit", "version") if getattr(a, k) is not None}
    if not changes:
        raise Fail("nothing to set (--kind/--unit/--version)")

    def fn(r):
        for k, v in changes.items():
            if v:
                r[k] = v
            else:
                r.pop(k, None)
    cat.update_side(e, fn)
    emit(a, public(e), lambda: print(f"{e['path']}: kind={e['kind']} unit={e['unit'] or '-'} version={e['version'] or '-'}"))
    return 0


def cmd_verify(a, cat):
    targets = cat.select(a, a.refs)
    results, worst = [], 0
    for e in targets:
        r = {"path": e["path"], "checks": []}
        raw, wr = e["raw"], e["wrapped"]
        state = "ok"
        if e["status"] == "untracked":
            r["result"] = "untracked (nothing recorded)"
            results.append(r)
            continue
        if e["status"] == "missing":
            r["result"] = "missing"
            worst = max(worst, 3) if worst != 1 else 1
            results.append(r)
            continue
        for part in (wr, raw):
            if not part["present"]:
                continue
            path = cat.root / "devices" / e["device"] / e["dir"] / part["name"]
            st = path.stat()
            want = [x for x in (part["sha256"], part["device_sha256"]) if x]
            if part["bytes"] is not None and part["bytes"] != st.st_size:
                r["checks"].append(f"{part['name']}: size {st.st_size} != recorded {part['bytes']}")
                state = "MISMATCH"
                continue
            sha, _ = hash_file(path, progress_printer(part["name"], st.st_size))
            cat.cache.put(path, st, sha)
            if not want:
                r["checks"].append(f"{part['name']}: {sha[:12]} (no hash recorded)")
            elif all(sha == w for w in want):
                r["checks"].append(f"{part['name']}: sha256 ok" + (" (matches on-device hash)" if part["device_sha256"] else ""))
            else:
                r["checks"].append(f"{part['name']}: sha256 {sha[:12]} != recorded {'/'.join(w[:12] for w in want)}")
                state = "MISMATCH"
        if wr["present"] and not raw["present"]:
            want = [x for x in (raw["sha256"], raw["device_sha256"]) if x]
            if a.raw and want:
                sha, n = hash_stream(e["abs"], e["codec"], progress=progress_printer(e["path"] + " (decompressed)", raw["bytes"]))
                okb = raw["bytes"] is None or raw["bytes"] == n
                if all(sha == w for w in want) and okb:
                    r["checks"].append(f"decompressed {raw['name']}: sha256 ok, {n} bytes")
                    if state == "ok":
                        state = "raw removed, content verified"
                else:
                    r["checks"].append(f"decompressed {raw['name']}: sha256 {sha[:12]} / {n} B != recorded {want[0][:12]} / {raw['bytes']}")
                    state = "MISMATCH"
            elif want and state == "ok":
                packed = ".zst" if e["codec"] == "zstd" else ".xz"
                state = (f"raw removed, {packed} verified" if wr["sha256"] else
                         f"raw removed, {packed} unrecorded (--raw checks content)")
        r["result"] = state
        if state == "MISMATCH":
            worst = 1
        results.append(r)
    cat.cache.save()

    def text():
        for r in results:
            print(f"{r['result']:28} {r['path']}")
            for c in r["checks"]:
                print(f"{'':28}   {c}")
    emit(a, results, text)
    return worst


def cmd_inspect(a, cat):
    targets = [e for e in cat.select(a, a.refs) if e["stored"]["present"]]
    out, rc = [], 0
    for e in targets:
        try:
            res = detect(e, cat.root, deep=a.deep)
        except Fail as exc:
            out.append({"path": e["path"], "error": str(exc)})
            rc = 1
            continue
        if not a.no_store:
            cat.update_side(e, lambda r: r.__setitem__("detect", res))
        out.append({"path": e["path"], "kind": e["side"].get("kind") or classify(e["name"], e["dir"], res), "detect": res})
    emit(a, out, lambda: [print(f"{o['path']}\n    {o.get('kind', '')}: {o['detect']['summary'] if 'detect' in o else 'ERROR ' + o['error']}") for o in out])
    return rc


def cmd_dupes(a, cat):
    groups = {}
    for e in cat.entries:
        key = e["content_sha256"] or (e["sha256"] and "packed:" + e["sha256"])
        if key:
            groups.setdefault(key, []).append(e)
    dup = {k: v for k, v in groups.items() if len(v) > 1}
    unhashed = sum(1 for e in cat.entries if not (e["content_sha256"] or e["sha256"]))
    obj = [{"content_sha256": k, "bytes": v[0]["bytes"], "fill": (v[0]["detect"] or {}).get("fill"),
            "images": [x["path"] for x in v]} for k, v in sorted(dup.items(), key=lambda kv: -(kv[1][0]["bytes"] or 0))]

    def text():
        for g in obj:
            fill = f"  [{g['fill']}]" if g["fill"] and g["fill"].startswith(("erased", "all zero")) else ""
            print(f"{g['content_sha256'][:12]}  {human(g['bytes'])}  ×{len(g['images'])}{fill}")
            for p in g["images"][:8]:
                print(f"    {p}")
            if len(g["images"]) > 8:
                print(f"    … {len(g['images']) - 8} more (--json lists all)")
        if unhashed:
            print(f"({unhashed} untracked image(s) have no hash yet: `ai-re-images scan --hash`)", file=sys.stderr)
    emit(a, obj, text)
    return 0


def in_image_dir(root, p):
    try:
        rel = p.resolve().relative_to((root / "devices").resolve())
    except ValueError:
        return False
    return len(rel.parts) >= 2 and rel.parts[1] in SCAN_DIRS


def cmd_export(a, cat):
    e = cat.resolve(a.ref)
    if not e["stored"]["present"]:
        raise Fail(f"{e['path']}: file not present", 1)
    out = Path(a.out).absolute()
    if out.exists():
        raise Fail(f"{out} exists; not overwriting")
    src_dir = e["abs"].parent.resolve()
    if (out.parent.resolve() == src_dir or in_image_dir(cat.root, out)) and not a.next_to_original:
        raise Fail(f"{out} is inside a device image directory; pick another place or pass --next-to-original")
    for tool in ["qemu-img"] + (["zstd"] if e["codec"] == "zstd" else ["xz"] if e["codec"] else []):
        if not shutil.which(tool):
            raise Fail(f"{tool} not on PATH (enter the devshell)", 1)
    out.parent.mkdir(parents=True, exist_ok=True)
    scratch = Path(a.scratch).absolute() if a.scratch else out.parent
    if scratch.resolve() == src_dir and not a.next_to_original:
        raise Fail("scratch dir is the original's directory; pick another --scratch")
    convert = ["qemu-img", "convert", "-m", "4", "-p" if sys.stderr.isatty() else "-q", "-f", "raw", "-O", "qcow2",
               "-c", "-o", "compression_type=zstd"]
    tmp_out = out.with_name(out.name + ".part")
    with contextlib.ExitStack() as stack:
        src = e["abs"]
        if e["codec"]:
            raw_bytes = e["raw"]["bytes"] or (zstd_frame_size(src) if e["codec"] == "zstd" else xz_size(src))
            free = shutil.disk_usage(scratch).free
            if raw_bytes and raw_bytes > free and not a.force_space:
                raise Fail(f"scratch {scratch} has {human(free)} free, raw image is {human(raw_bytes)} "
                           "(written sparse, so it may fit: --force-space to try)", 1)
            td = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix=".ai-re-images-", dir=scratch)))
            src = td / "raw.img"
            with open(src, "wb") as f:
                def sink(chunk, f=f):
                    if chunk.count(0) == len(chunk):
                        f.seek(len(chunk), 1)
                    else:
                        f.write(chunk)
                sha, n = hash_stream(e["abs"], e["codec"], sink, progress_printer("decompress", raw_bytes))
                f.truncate(n)
            want = e["raw"]["sha256"] or e["raw"]["device_sha256"]
            if want and sha != want:
                raise Fail(f"decompressed content sha256 {sha[:12]} != recorded {want[:12]}; not exporting", 1)
            if sys.stderr.isatty():
                print(file=sys.stderr)
        r = subprocess.run(convert + [str(src), str(tmp_out)])
        if r.returncode != 0:
            tmp_out.unlink(missing_ok=True)
            raise Fail("qemu-img convert failed", 1)
        os.replace(tmp_out, out)
    info = {"out": str(out), "bytes": out.stat().st_size, "source": e["path"],
            "attach": [f"sudo modprobe nbd && sudo qemu-nbd --read-only --connect=/dev/nbd0 {out}",
                       "sudo mount -o ro,noload /dev/nbd0p1 /mnt   # noload: no ext3/4 journal replay",
                       "sudo umount /mnt; sudo qemu-nbd --disconnect /dev/nbd0",
                       f"nbdkit --readonly --filter=qcow2dec file {out}   # no root; serves NBD on :10809"]}
    emit(a, info, lambda: print(f"wrote {out} ({human(info['bytes'])}) from {e['path']}\nattach read-only (not done for you):\n  "
                                + "\n  ".join(info["attach"])))
    return 0


# ---------------------------------------------------------------- main

def main(argv=None):
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--json", action="store_true", help="machine-readable output")
    common.add_argument("--root", help="ai_re directory (default: found from the cwd or AI_RE_ROOT)")
    common.add_argument("--device", help="limit to one device directory")
    filt = argparse.ArgumentParser(add_help=False)
    for f in ("unit", "kind", "text", "status"):
        filt.add_argument(f"--{f}")
    filt.add_argument("--tag", action="append", default=[])

    ap = argparse.ArgumentParser(prog="ai-re-images", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("scan", parents=[common], help="read manifests, find untracked/missing images")
    p.add_argument("--hash", action="store_true", help="sha256 untracked files (cached outside the repo)")
    p = sub.add_parser("list", parents=[common, filt], help="list images")
    p.add_argument("--sort", choices=sorted(SORTS), default="device")
    p.add_argument("-r", "--reverse", action="store_true")
    p = sub.add_parser("show", parents=[common], help="everything about one image")
    p.add_argument("ref")
    for name in ("tag", "untag"):
        p = sub.add_parser(name, parents=[common], help=f"{name} an image")
        p.add_argument("ref")
        p.add_argument("tags", nargs="+")
    p = sub.add_parser("note", parents=[common], help="append a dated note")
    p.add_argument("ref")
    p.add_argument("text", nargs="+")
    p = sub.add_parser("set", parents=[common], help="override kind/unit/version ('' clears)")
    p.add_argument("ref")
    p.add_argument("--kind")
    p.add_argument("--unit")
    p.add_argument("--version")
    p = sub.add_parser("verify", parents=[common, filt], help="re-hash and compare with recorded hashes")
    p.add_argument("refs", nargs="*")
    p.add_argument("--raw", action="store_true", help="also hash the decompressed content of .zst/.xz")
    p = sub.add_parser("inspect", parents=[common, filt], help="detect partition tables, filesystems, headers")
    p.add_argument("refs", nargs="*")
    p.add_argument("--deep", action="store_true", help=f"read compressed streams past {human(STREAM_LIMIT)} to reach later partitions")
    p.add_argument("--no-store", action="store_true", help="print only, don't write the sidecar")
    sub.add_parser("dupes", parents=[common], help="images with identical content")
    p = sub.add_parser("export-qcow2", parents=[common], help="convert to zstd qcow2 for read-only random access")
    p.add_argument("ref")
    p.add_argument("out")
    p.add_argument("--scratch", help="where a .zst is decompressed first (default: OUT's directory)")
    p.add_argument("--force-space", action="store_true")
    p.add_argument("--next-to-original", action="store_true", help="allow OUT/scratch in the image's own directory")
    a = ap.parse_args(argv)
    try:
        root = find_root(a.root)
        cat = Catalog(root, [a.device] if a.device else None)
        fn = {"scan": cmd_scan, "list": cmd_list, "show": cmd_show, "tag": cmd_tag, "untag": cmd_tag,
              "note": cmd_note, "set": cmd_set, "verify": cmd_verify, "inspect": cmd_inspect,
              "dupes": cmd_dupes, "export-qcow2": cmd_export}[a.cmd]
        return fn(a, cat)
    except Fail as exc:
        print(f"ai-re-images: {exc}", file=sys.stderr)
        return exc.code
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
