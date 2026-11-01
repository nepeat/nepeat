"""Pinned ONIE build-target discovery. Makefiles are data, never executed."""
import hashlib
import json
import re
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REVISION = "ce480be7094deef7198ee4d50af8427ea89b93e3"
MACHINE_TREE = "c32bc41b496da71f2ffcc1bf29c6619333635516"
REPO = "https://github.com/opencomputeproject/onie"
VENDORS = {"accton": "Accton", "celestica": "Celestica"}


def target_files(tree):
    if tree.get("truncated") is not False:
        raise ValueError("ONIE index is truncated or has no completeness flag")
    rows = {}
    for entry in tree["tree"]:
        path = entry["path"]
        parts = path.split("/")
        if len(parts)!=3 or parts[0] not in VENDORS or parts[2]!="machine.make":
            continue
        if entry["type"]!="blob" or entry.get("mode") not in ("100644","100755"):
            raise ValueError("Unexpected ONIE target file type: " + path)
        if not re.fullmatch(r"[a-zA-Z0-9_-]+",parts[1]):
            raise ValueError("Invalid ONIE target name: " + path)
        if path in rows:
            raise ValueError("Duplicate ONIE index path: " + path)
        rows[path]=entry
    if not rows:
        raise ValueError("ONIE index contains no target-vendor machine files")
    return rows


def blob_sha(data):
    return hashlib.sha1(b"blob "+str(len(data)).encode()+b"\0"+data).hexdigest()


def collect(raw, fetch, refresh=False):
    """Fetch a bounded set of public files, and publish only a complete bundle."""
    raw=Path(raw)
    tree_path=fetch("https://api.github.com/repos/opencomputeproject/onie/git/trees/"+
                    MACHINE_TREE+"?recursive=1",raw/"onie-machine-tree.json",refresh,max_bytes=8_000_000)
    tree=json.loads(tree_path.read_text())
    if tree.get("sha")!=MACHINE_TREE:
        raise ValueError("ONIE machine tree does not match pinned snapshot")
    expected=target_files(tree)

    def one(item):
        relpath, entry=item
        url="https://raw.githubusercontent.com/opencomputeproject/onie/"+REVISION+"/machine/"+relpath
        # Paths were constrained to three safe components above.
        path=fetch(url,raw/"onie"/REVISION/relpath,refresh,max_bytes=1_000_000)
        body=path.read_bytes()
        if blob_sha(body)!=entry["sha"]:
            raise ValueError("ONIE blob checksum mismatch: "+relpath)
        return {"path":relpath,"git_blob_sha":entry["sha"],"text":body.decode("utf-8")}

    with ThreadPoolExecutor(max_workers=4) as pool:
        files=list(pool.map(one,sorted(expected.items())))
    bundle={"revision":REVISION,"tree":tree,"files":files}
    path=raw/"onie-platforms.json"
    tmp=path.with_suffix(".json.part")
    tmp.write_text(json.dumps(bundle,ensure_ascii=False,indent=2))
    tmp.replace(path)
    return path


def import_bundle(c,path):
    bundle=json.loads(Path(path).read_text())
    revision=bundle["revision"]
    if not re.fullmatch(r"[0-9a-f]{40}",revision):
        raise ValueError("ONIE revision must be an immutable Git SHA")
    expected=target_files(bundle["tree"])
    files=bundle["files"]
    paths=[f["path"] for f in files]
    if len(set(paths))!=len(paths) or set(paths)!=set(expected):
        raise ValueError("ONIE bundle is incomplete or contains duplicate/unexpected files")
    # Validate every file before changing database state.
    for f in files:
        if f["git_blob_sha"]!=expected[f["path"]]["sha"] or blob_sha(f["text"].encode())!=f["git_blob_sha"]:
            raise ValueError("ONIE blob checksum mismatch: "+f["path"])
    c.source("onie",REPO,"project","ONIE GPL-2.0; source fragments and factual build metadata",
             "Complete Accton/Celestica machine.make file set in pinned ONIE machine tree. Build targets may be families or internal identifiers; no claim of retail availability or current NOS support.",path,revision)
    for f in files:
        vendor_key,platform,_=f["path"].split("/")
        vendor=VENDORS[vendor_key]
        first_comment=next((line.lstrip("# ") for line in f["text"].splitlines() if line.startswith("# ")),"")
        # Merge only an explicit complete AS model in a source title comment.
        # Other build targets keep a separate identity until equivalence is reviewed.
        match=re.fullmatch(r"(?:Makefile fragment for )?Accton\s+(AS\d{4,5}(?:[-_][A-Z0-9]+)+)(?:\s+Switch)?",first_comment,re.I)
        # Some upstream comments were copied from another platform. Require
        # agreement with the directory before mapping to a marketed model.
        if match and platform.removeprefix("accton_").lower()!=match.group(1).replace("-","_").lower():
            match=None
        model=match.group(1).upper().replace("_","-") if match else "onie:"+platform
        facts=[("identity_basis","Explicit ONIE source title model" if match else "ONIE build target; marketed-model mapping unverified"),
               ("source_title",first_comment),
               ("software_evidence","Build definition in pinned ONIE repository. Not proof of installed firmware, available NOS images, or per-unit boot compatibility.")]
        # Preserve simple literal settings only, without interpreting conditionals,
        # substitutions, included files, or shell functions.
        for name in ("ONIE_ARCH","SWITCH_ASIC_VENDOR","CONSOLE_DEV","UEFI_ENABLE","VENDOR_ID"):
            for value in re.findall(r"^\s*"+name+r"\s*(?:\?=|:=|=)\s*([A-Za-z0-9_.-]+)\s*(?:#.*)?$",f["text"],re.M):
                facts.append(("build_"+name.lower(),value))
        identifiers=[("onie_platform",platform)]
        c.add("onie",f["path"],REPO+"/blob/"+revision+"/machine/"+f["path"],vendor,model,
              vendor+" "+(model if match else "ONIE target "+platform),"network platform",f,
              [(vendor,"platform_vendor")],identifiers,facts)
    c.finish("onie",len(files))
    return len(files)
