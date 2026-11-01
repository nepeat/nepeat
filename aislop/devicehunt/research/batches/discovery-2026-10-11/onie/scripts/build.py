#!/usr/bin/env python3
"""capture + extract + build curated candidate for ONIE machine dirs. run with python3 -I."""
import hashlib, json, os, re, shutil, sys

CLONE = "/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/opencomputeproject/onie-clone"
RAW = "/home/erin/nepeat/aislop/devicehunt/data/raw/discovery-2026-10-11-onie"
RES = "/home/erin/nepeat/aislop/devicehunt/research/batches/discovery-2026-10-11/onie"
COMMIT = "ce480be7094deef7198ee4d50af8427ea89b93e3"
CLONE_UTC = "2026-10-11T09:01:15Z"
REPO = "https://github.com/opencomputeproject/onie"
VENDORS = ["accton", "alphanetworks", "celestica", "ingrasys", "inventec", "quanta", "wiwynn"]
BASE = f"opencomputeproject/onie/{COMMIT}"

def sha(b): return hashlib.sha256(b).hexdigest()

# ---- capture -----------------------------------------------------------
OVERLAP = set()
EXIST = json.load(open('/home/erin/nepeat/aislop/devicehunt/data/raw/onie-platforms.json'))
assert EXIST['revision'] == COMMIT
for f in EXIST['files']:
    m_ = re.match(r"^(accton|celestica|[^/]+)/([^/]+)/machine\.make$", f['path'])
    if m_: OVERLAP.add(m_.group(2))
machines = []
for v in VENDORS:
    vd_ = f"{RAW}/{BASE}/machine/{v}"
    for n_ in sorted(os.listdir(vd_)):
        if os.path.isfile(f"{vd_}/{n_}/machine.make"): machines.append((v, n_))
manifest = json.load(open(f"{RAW}/capture-manifest.json"))
man_files = manifest['files']

# ---- extract -----------------------------------------------------------
def lines_of(rel):
    return open(f"{RAW}/{BASE}/{rel}", "rb").read().decode("utf-8", "replace").split("\n")

VARS = ["ONIE_ARCH","SWITCH_ASIC_VENDOR","VENDOR_REV","MACHINE_REV","VENDOR_ID","VENDOR_VERSION",
        "CONSOLE_DEV","CONSOLE_SPEED","CONSOLE_FLAG","UEFI_ENABLE","PXE_EFI64_ENABLE","SECURE_BOOT_ENABLE",
        "SERIAL_CONSOLE_ENABLE","PARTITION_TYPE","LINUX_VERSION","LINUX_MINOR_VERSION","GCC_VERSION",
        "I2CTOOLS_ENABLE","I2CTOOLS_SYSEEPROM","IPMITOOL_ENABLE","FIRMWARE_UPDATE_ENABLE","KERNEL_DTB",
        "UBOOT_MACHINE","UBOOT_VERSION","EXTRA_CMDLINE_LINUX","ROOTDELAY","GRUB_TIMEOUT","SKIP_ETHMGMT_MACS"]
ASSIGN = re.compile(r"^(\s*)([A-Z][A-Z0-9_]*)\s*(\?=|:=|\+=|=)\s*(.*?)\s*$")

def fname(var): return "onie_arch" if var == "ONIE_ARCH" else "onie_" + var.lower()

def parse_machine_make(vd, n):
    rel = f"machine/{vd}/{n}/machine.make"
    L = lines_of(rel)
    facts = []
    cond_depth = 0
    seen = {}
    for i, ln in enumerate(L, 1):
        s = ln.strip()
        if re.match(r"^(ifeq|ifneq|ifdef|ifndef)\b", s): cond_depth += 1; continue
        if re.match(r"^endif\b", s): cond_depth -= 1; continue
        if s.startswith("#"): continue
        m = ASSIGN.match(ln)
        if not m or m.group(2) not in VARS: continue
        var, op, val = m.group(2), m.group(3), m.group(4)
        val = re.sub(r"\s+#.*$", "", val).strip()
        tag = " [inside conditional]" if cond_depth > 0 else ""
        seen.setdefault(var, []).append(f"{val}{'' if op in ('=','?=') else ' [op '+op+']'}{tag} (machine.make:{i})")
    for var in VARS:
        if var in seen:
            facts.append([fname(var), "; ".join(seen[var])])
    # comments
    hdr, copy, spdx, cpu = [], [], [], []
    for i, ln in enumerate(L, 1):
        s = ln.strip()
        if not s.startswith("#"):
            if s and not hdr and not copy: pass
            if s: break
            continue
        t = s.lstrip("#").strip()
        if not t or re.fullmatch(r"[-=*#\s]+", t): continue
        if re.search(r"copyright", t, re.I):
            if '<' not in t and '@' not in t and re.search(r"\b(INC|CORP|LTD|CO)\b\.?|COMPUTER", t, re.I):
                mm = re.sub(r"^.*?Copyright\s*(\(C\))?\s*[\d,\-\s]*", "", t, flags=re.I).strip()
                copy.append(f"{mm} (machine.make:{i})")
        elif re.search(r"SPDX-License-Identifier", t): spdx.append(f"{t} (machine.make:{i})")
        elif re.search(r"Vendor's version number can|Available variable is 'VENDOR_VERSION'|^e\.g\.,$|^VENDOR_VERSION = \.00\.01$", t): pass
        else: hdr.append(f"{t} (machine.make:{i})")
    for i, ln in enumerate(L, 1):
        s = ln.strip()
        if s.startswith("#") and re.search(r"\bCPU\b|\bcpu\b|\bCOM-Express\b|COMe", s):
            cpu.append(f"{s.lstrip('#').strip()} (machine.make:{i})")
    if hdr: facts.insert(0, ["onie_machine_make_header_comments", json.dumps(hdr[:12], ensure_ascii=False)])
    if copy: facts.append(["onie_copyright_company_as_printed", "; ".join(copy[:3])])
    if spdx: facts.append(["onie_machine_make_spdx", json.dumps(spdx[:3], ensure_ascii=False)])
    if cpu: facts.append(["onie_machine_make_cpu_comments", json.dumps(cpu[:8], ensure_ascii=False)])
    # vendor id comment: comment block directly above VENDOR_ID
    for i, ln in enumerate(L, 1):
        m = ASSIGN.match(ln)
        if m and m.group(2) == "VENDOR_ID":
            j = i - 2; c = []
            while j >= 0 and L[j].strip().startswith("#"):
                t = L[j].strip().lstrip("#").strip()
                if t and not t.startswith("Vendor ID --") and "iana.org/assignments/enterprise-numbers" not in t:
                    c.insert(0, f"{t} (machine.make:{j+1})")
                j -= 1
            if c: facts.append(["onie_vendor_id_code_comment", "code comment(s) above VENDOR_ID, not a role claim: " + json.dumps(c, ensure_ascii=False)])
            break
    return facts

PLAT = re.compile(r"[A-Za-z0-9_.]*[a-z0-9]+_[a-z0-9_]+-r\d+[A-Za-z0-9_]*")

def parse_install(vd, n, relname):
    rel = f"machine/{vd}/{n}/{relname}"
    L = lines_of(rel)
    facts = []
    title = []
    for i, ln in enumerate(L[:6], 1):
        s = ln.strip()
        if re.match(r"^(Cross-Compiling|Change directories)", s): break
        if s and not re.fullmatch(r"[=\-~*\s]+", s):
            title.append(f"{s} ({relname}:{i})")
    if title: facts.append(["onie_install_heading_lines", json.dumps(title[:6], ensure_ascii=False)])
    for i, ln in enumerate(L, 1):
        if re.search(r"MACHINEROOT=(\S+)\s+MACHINE=(\S+)", ln):
            facts.append(["onie_install_build_machine_arg", f"{ln.rstrip()} ({relname}:{i})"]); break
    ren, comp, plats = [], [], {}
    for i, ln in enumerate(L, 1):
        if re.search(r"old machine name|has been renamed|renamed to", ln, re.I): ren.append(f"{ln.strip()} ({relname}:{i})")
        m = re.match(r"^\s*-\s+([a-z0-9_]+-r\d+\S*)\s*$", ln)
        if m: comp.append(f"{m.group(1)} ({relname}:{i})")
        for t in PLAT.findall(ln):
            plats.setdefault(t, i)
    if ren: facts.append(["onie_install_rename_note", json.dumps(ren, ensure_ascii=False)])
    if comp: facts.append(["onie_install_compatible_machines", json.dumps(comp, ensure_ascii=False)])
    if plats:
        facts.append(["onie_install_platform_strings", json.dumps([f"{k} ({relname}:{v})" for k, v in list(plats.items())[:12]], ensure_ascii=False)])
    prods = []
    for i, ln in enumerate(L, 1):
        m = re.match(r"^\s*-\s+(\S.*?)\s+\((cpu module|cpu on board)\)\s*$", ln)
        if m: prods.append(f"{ln.rstrip()} ({relname}:{i})")
    if prods: facts.append(["onie_install_supported_platform_lines", json.dumps(prods, ensure_ascii=False)])
    # lines that print a CPU/processor name (console dumps), vs other board/CPU-family text
    hl = {int(re.search(r":(\d+)\)$", x).group(1)) for x in title}
    cpu, other = [], []
    for i, ln in enumerate(L, 1):
        if re.search(r"Rangeley|Broadwell|Denverton|Xeon|Atom|P2020|COM-?E", ln) and not re.search(r"-rw-|\.initrd|\.vmlinuz", ln):
            t = ln.strip()
            if not t or re.fullmatch(r"[=\-\s]+", t) or i in hl: continue
            if re.match(r"^\s*(CPU:|Processor Type:)", ln): cpu.append(f"{ln.rstrip()} ({relname}:{i})")
            else: other.append(f"{ln.rstrip()} ({relname}:{i})")
    if cpu: facts.append(["onie_install_cpu_lines", json.dumps(cpu[:6], ensure_ascii=False)])
    if other: facts.append(["onie_install_heading_adjacent_lines", json.dumps(other[:6], ensure_ascii=False)])
    return facts

def parse_readme(vd, n, relname):
    rel = f"machine/{vd}/{n}/{relname}"
    L = lines_of(rel)
    facts = []
    h = [f"{ln.strip()} ({relname}:{i})" for i, ln in enumerate(L, 1) if ln.startswith("#")][:6]
    if h: facts.append(["onie_readme_headings", json.dumps(h, ensure_ascii=False)])
    rows = [f"{ln.strip()} ({relname}:{i})" for i, ln in enumerate(L, 1) if re.match(r"^\|\s*x86_64-\S+\s*\|", ln) or re.match(r"^\|\s*(armv|powerpc)\S*-\S+\s*\|", ln)]
    if rows: facts.append(["onie_readme_platform_table_rows", json.dumps(rows[:12], ensure_ascii=False)])
    bl = [f"{ln.strip()} ({relname}:{i})" for i, ln in enumerate(L, 1) if "belongs to" in ln]
    if bl: facts.append(["onie_readme_model_line", json.dumps(bl[:3], ensure_ascii=False)])
    return facts

def parse_genconf(vd, n):
    relname = "rootconf/sysroot-lib-onie/gen-config-platform"
    L = lines_of(f"machine/{vd}/{n}/{relname}")
    sel = [f"{ln.strip()} ({relname}:{i})" for i, ln in enumerate(L, 1) if re.search(r"onie_machine|onie_switch_asic|live_machine=|platform=", ln)]
    return [["onie_gen_config_platform_lines", json.dumps(sel[:8], ensure_ascii=False)]] if sel else []

# vendor-level readme: lines naming each machine dir
vendor_readme = {}
for v in VENDORS:
    p = f"machine/{v}/README.md"
    if os.path.isfile(f"{RAW}/{BASE}/{p}"):
        vendor_readme[v] = (p, lines_of(p))

def vendor_readme_facts(v, n):
    if v not in vendor_readme: return []
    p, L = vendor_readme[v]
    out = []
    for i, ln in enumerate(L, 1):
        if (f"[{n}](https://github.com/opencomputeproject/onie/tree/master/machine/{v}/{n})" in ln) or (f"[machine/{v}/{n}](https://github.com/opencomputeproject/onie/tree/master/machine/{v}/{n})" in ln):
            if v == "quanta":
                # Path line: report the preceding "### - " heading
                j = i - 2
                while j >= 0 and not L[j].startswith("###"): j -= 1
                out.append(f"{L[j].strip()} (README.md:{j+1}); path line (README.md:{i})")
            else:
                out.append(f"{ln.strip()} (README.md:{i})")
    return [["onie_vendor_readme_entries", json.dumps(out, ensure_ascii=False)]] if out else []

sources, devices, per = [], [], {}
for v, n in machines:
    d = f"machine/{v}/{n}"
    facts = [["onie_repo_commit", f"opencomputeproject/onie@{COMMIT}"],
             ["onie_vendor_dir_as_printed", v],
             ["onie_machine_dir_as_printed", f"machine/{v}/{n}"]]
    facts += parse_machine_make(v, n)
    cap = sorted(os.listdir(f"{RAW}/{BASE}/{d}"))
    for f in cap:
        if f.startswith("INSTALL"): facts += parse_install(v, n, f)
        elif f.startswith("README"): facts += parse_readme(v, n, f)
    if os.path.isfile(f"{RAW}/{BASE}/{d}/rootconf/sysroot-lib-onie/gen-config-platform"): facts += parse_genconf(v, n)
    facts += vendor_readme_facts(v, n)
    facts.append(["onie_captured_files", json.dumps([f"{d}/{f}" if os.path.isfile(f"{RAW}/{BASE}/{d}/{f}") else f"{d}/{f}/sysroot-lib-onie/gen-config-platform" for f in cap])])
    # dedupe fact names
    names = [f[0] for f in facts]; assert len(names) == len(set(names)), (n, names)
    for f in facts: assert len(f[1]) < 4000, (n, f[0], len(f[1]))
    per[n] = {"vendor_dir": v, "in_existing_onie_source": n in OVERLAP, "facts": facts}
    if n in OVERLAP: continue
    sid = f"onie-machine-{n}"
    sources.append({"id": sid, "url": f"{REPO}/tree/{COMMIT}/{d}", "tier": "project",
        "license": "GPL-2.0 (repo COPYING) / upstream rights; factual extraction",
        "coverage": f"ONIE machine definition at opencomputeproject/onie commit {COMMIT}; a software build/reference entry, not a hardware datasheet, vendor support statement, installed configuration or proof of a product's existence or variant. Facts are literal machine.make assignments and INSTALL/README text with file:line; a fact is omitted when the files lack it. Vendor directory names and IANA/copyright comments are labels, not company roles.",
        "snapshot": f"{BASE}/{d}/machine.make"})
    devices.append({"source": sid, "key": n, "brand": "ONIE machine", "model": n, "title": f"ONIE machine {n}",
        "kind": "network switch ONIE machine definition (software reference)", "affiliations": [],
        "identifiers": [["onie_machine", n]], "facts": facts})

cand = {"sources": sources, "devices": devices}
json.dump(cand, open(f"{RAW}/onie-machine-facts-2026-10-11.json", "w"), indent=1, ensure_ascii=False)
os.makedirs(RES, exist_ok=True)
json.dump(per, open(f"{RES}/extraction.json", "w"), indent=1, ensure_ascii=False)
print(len(machines), "machines;", len(devices), "candidate;", len(OVERLAP), "overlap;", len(man_files), "files;", sum(len(d['facts']) for d in devices), "facts")
