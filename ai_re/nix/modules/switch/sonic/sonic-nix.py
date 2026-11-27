#!/usr/bin/env python3
"""sonic-nix: CONFIG_DB ownership between NixOS and the SONiC CLI.

Rules: devices/sonic-nixos/config-ownership.md (ai_re). Inputs are generated
by the NixOS module (nix/modules/switch/sonic) and passed via the
environment of the wrapper:

  SONIC_NIX_BASE   Nix base config (config_db.json form), YANG-validated at build
  SONIC_NIX_MODES  {"TABLE": {"key": {"field": "enforce"|"seed"}}} for every base field
  SONIC_NIX_META   {"hostTables": [...], "hostFields": [...], "ownedKeyTables": [...],
                    "userFile": ..., "runDir": ..., "yangDir": ..., "stopUnits": [...]}

Commands:
  load [--user-file F] [--no-validate]   boot: merge, validate, flush+write CONFIG_DB
  reload [--user-file F] [--no-service-restart]
                                         "config reload": stop sonic.target and its units, load, start
  apply                                  nixos-rebuild switch: write changed enforce/seed keys live
  diff [--json] [--all]                  Nix vs running CONFIG_DB vs saved file
  watch                                  keep conflicts.json, /etc/issue and motd current
"""
import argparse, copy, json, os, subprocess, sys, syslog, time

ENFORCE, SEED = "enforce", "seed"


def env_json(name):
    with open(os.environ[name]) as f:
        return json.load(f)


BASE = env_json("SONIC_NIX_BASE")
MODES = env_json("SONIC_NIX_MODES")
META = env_json("SONIC_NIX_META")
RUN = META["runDir"]
USER_FILE = META["userFile"]


def log(prio, msg):
    syslog.syslog(prio, msg)
    if "JOURNAL_STREAM" not in os.environ:   # under systemd syslog alone reaches the journal
        print(msg, file=sys.stderr if prio <= syslog.LOG_WARNING else sys.stdout, flush=True)


def warn(msg):
    log(syslog.LOG_WARNING, msg)


def info(msg):
    log(syslog.LOG_INFO, msg)


def fmt(v):
    if v is None:
        return "(absent)"
    if isinstance(v, list):
        return ",".join(v)
    if isinstance(v, dict):
        return json.dumps(v, sort_keys=True)
    return str(v)


def norm(v):
    # cfggen writes lists for "field@" values; compare lists and strings alike.
    return fmt(v) if isinstance(v, list) else v


def key_parts(key):
    return key.split("|")


# ---------------------------------------------------------------- merge rules

def is_host_table(table):
    for h in META["hostTables"]:
        if h.endswith("*") and table.startswith(h[:-1]) or table == h:
            return True
    return False


def host_field(table, key, field):
    return "%s|%s|%s" % (table, key, field) in META["hostFields"]


def strip_host(cfg, source, conflicts):
    """Drop host tables / fields (NixOS owns them) from a user layer."""
    out = {}
    for table, entries in cfg.items():
        if is_host_table(table):
            conflicts.append(dict(table=table, key="*", field=None, nix="(NixOS host config)",
                                  current=fmt(sorted(entries)), source=source, kind="host"))
            continue
        out[table] = {}
        for key, fields in entries.items():
            if not isinstance(fields, dict):
                continue
            kept = {}
            for f, v in fields.items():
                if host_field(table, key, f):
                    conflicts.append(dict(table=table, key=key, field=f, nix="(NixOS host config)",
                                          current=fmt(v), source=source, kind="host"))
                else:
                    kept[f] = v
            out[table][key] = kept
    return out


def stale_entries(cfg, source, conflicts):
    """Entries Nix does not declare in key-owned tables (PORT: the port set is
    hardware), plus entries elsewhere whose key names a removed port."""
    removed = set()
    for table in META["ownedKeyTables"]:
        for key in list(cfg.get(table, {})):
            if key not in BASE.get(table, {}):
                conflicts.append(dict(table=table, key=key, field=None, nix="(absent)",
                                      current=fmt(cfg[table][key]), source=source, kind="stale"))
                del cfg[table][key]
                removed.add(key)
    if removed:
        for table, entries in cfg.items():
            for key in list(entries):
                if removed & set(key_parts(key)):
                    conflicts.append(dict(table=table, key=key, field=None, nix="(absent: port removed)",
                                          current=fmt(entries[key]), source=source, kind="stale"))
                    del entries[key]
    return cfg


def merge(user, source):
    """Nix base + one user layer -> (result, conflicts)."""
    conflicts = []
    result = strip_host(copy.deepcopy(user), source, conflicts)
    stale_entries(result, source, conflicts)
    for table, entries in BASE.items():
        rt = result.setdefault(table, {})
        for key, fields in entries.items():
            re_ = rt.setdefault(key, {})
            for f, v in fields.items():
                mode = MODES[table][key][f]
                uv = re_.get(f)
                if mode == ENFORCE:
                    if uv is not None and norm(uv) != norm(v):
                        conflicts.append(dict(table=table, key=key, field=f, nix=fmt(v),
                                              current=fmt(uv), source=source, kind="enforce"))
                    re_[f] = v
                elif uv is None:
                    re_[f] = v
    return {t: e for t, e in result.items() if e}, conflicts


def enforce_conflicts(cfg, source):
    """Enforced fields whose value in cfg differs from Nix (no changes made)."""
    out = []
    for table, entries in BASE.items():
        for key, fields in entries.items():
            have = cfg.get(table, {}).get(key, {})
            for f, v in fields.items():
                if MODES[table][key][f] != ENFORCE:
                    continue
                cv = have.get(f)
                if norm(cv) != norm(v):
                    out.append(dict(table=table, key=key, field=f, nix=fmt(v), current=fmt(cv),
                                    source=source, kind="enforce"))
    return out


# ---------------------------------------------------------------- CONFIG_DB

def configdb(wait=30):
    from swsscommon.swsscommon import ConfigDBConnector
    deadline = time.time() + wait
    while True:
        try:
            db = ConfigDBConnector()
            db.db_connect("CONFIG_DB", False, False)
            return db
        except Exception as e:  # redis not up yet
            if time.time() > deadline:
                raise
            time.sleep(0.5)


def live_config(db):
    raw = db.get_config()
    out = {}
    for table, entries in raw.items():
        out[table] = {}
        for key, fields in entries.items():
            k = "|".join(key) if isinstance(key, tuple) else key
            out[table][k] = dict(fields)
    return out


def write_config(db, cfg, flush):
    client = db.get_redis_client(db.CONFIG_DB)
    if flush:
        client.flushdb()
    db.mod_config(cfg)
    client.set(db.INIT_INDICATOR, 1)


def set_field(db, table, key, field, value):
    entry = db.get_entry(table, key) or {}
    entry[field] = value
    db.set_entry(table, key, entry)


# ---------------------------------------------------------------- files

def read_user(path, conflicts):
    if not path or not os.path.exists(path):
        return {}
    try:
        with open(path) as f:
            data = json.load(f)
        if not isinstance(data, dict):
            raise ValueError("not a JSON object")
        return data
    except Exception as e:
        warn("sonic-nix: ignoring unreadable saved config %s: %s" % (path, e))
        conflicts.append(dict(table="*", key=path, field=None, nix="(Nix base only)",
                              current="unreadable: %s" % e, source="saved", kind="invalid"))
        return {}


def yang_validate(cfg):
    """Returns a list of problems (empty = valid)."""
    try:
        import sonic_yang
        sy = sonic_yang.SonicYang(META["yangDir"], print_log_enabled=False)
        sy.loadYangModel()
        sy.loadData(configdbJson=copy.deepcopy(cfg))
        sy.validate_data_tree()
    except Exception as e:
        return [str(e)]
    missing = sorted(t for t in sy.tablesWithOutYang if t != "bgpraw")
    return ["tables without YANG model (not validated): " + ", ".join(missing)] if missing else []


def atomic_write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    # A name of this process's own: sonic-nix-apply and sonic-nix-watch write the same files and
    # can start in the same second; with one shared ".tmp" one renamed it away under the other
    # (FileNotFoundError at boot, AS7772 2026-11-23).
    tmp = "%s.%d.tmp" % (path, os.getpid())
    try:
        with open(tmp, "w") as f:
            f.write(text)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def label(c):
    where = c["table"] if c["key"] == "*" else "%s|%s" % (c["table"], c["key"])
    return where + ("" if c["field"] is None else " " + c["field"])


def describe(c):
    saved = "saved" in c["source"]
    where = ("in " + USER_FILE) if saved else "not saved"
    if c["kind"] == "enforce":
        then = "Nix value loaded at boot" if c["source"] == "saved" else "reverted to Nix at the next boot / nixos-rebuild switch"
        return "%s: Nix %s, current %s (%s; %s)" % (label(c), c["nix"], c["current"], then, where)
    if c["kind"] == "host":
        return "%s (%s): NixOS host config, never loaded into CONFIG_DB (%s)" % (label(c), c["current"], where)
    if c["kind"] == "stale":
        return "%s: not a Nix port any more (breakout changed?), dropped (%s)" % (label(c), where)
    return "%s: %s" % (label(c), c["current"])


def summary(conflicts, notes):
    if not conflicts and not notes:
        return ""
    lines = ["SONiC config (NixOS): %d setting(s) differ from the Nix configuration and will go away:" % len(conflicts)]
    for c in conflicts[:20]:
        lines.append("  " + describe(c))
    if len(conflicts) > 20:
        lines.append("  ... and %d more" % (len(conflicts) - 20))
    lines += ["  " + n for n in notes]
    lines.append("Full list: sonic-nix-diff. Keep a value: declare it in Nix (or mode seed); drop it: config save.")
    return "\n".join(lines) + "\n"


def publish(conflicts, notes=()):
    notes = list(notes)
    state = dict(time=int(time.time()), conflicts=conflicts, notes=notes)
    atomic_write(os.path.join(RUN, "conflicts.json"), json.dumps(state, indent=2, sort_keys=True) + "\n")
    text = summary(conflicts, notes)
    # agetty: \ escapes are interpreted in issue files; keep the text plain.
    atomic_write(os.path.join(RUN, "issue"), text.replace("\\", "\\\\") + ("\n" if text else ""))
    atomic_write(os.path.join(RUN, "motd"), text)
    subprocess.run(["agetty", "--reload"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)


def boot_conflicts():
    try:
        with open(os.path.join(RUN, "boot-conflicts.json")) as f:
            return json.load(f)
    except Exception:
        return dict(conflicts=[], notes=[])


def combine(boot, runtime, saved):
    """Merge the conflict lists of the boot merge (saved values Nix replaced),
    the running CONFIG_DB and the saved file into one per field."""
    by = {}
    for c in boot + saved + runtime:
        k = (c["table"], c["key"], c["field"], c["kind"])
        if k in by:
            old = by[k]
            if c["source"] not in old["source"]:
                old["source"] = old["source"] + "+" + c["source"]
            if c["source"] == "runtime":
                old["current"] = c["current"]
        else:
            by[k] = dict(c)
    return sorted(by.values(), key=lambda c: (c["table"], c["key"], c["field"] or ""))


def current_conflicts(db):
    live = live_config(db)
    runtime = enforce_conflicts(live, "runtime")
    hostc = []
    strip_host(live, "runtime", hostc)
    conf = []
    saved = read_user(USER_FILE, conf)
    savedc = []
    if saved:
        s = strip_host(saved, "saved", savedc)
        stale_entries(s, "saved", savedc)
        savedc += [c for c in enforce_conflicts(s, "saved")
                   if s.get(c["table"], {}).get(c["key"], {}).get(c["field"]) is not None]
    b = boot_conflicts()
    # A boot conflict stays pending while the saved file still holds it.
    still = {(c["table"], c["key"], c["field"]) for c in savedc}
    bootc = [c for c in b["conflicts"] if (c["table"], c["key"], c["field"]) in still]
    return combine(bootc, runtime + hostc, savedc + conf), live, b.get("notes", [])


# ---------------------------------------------------------------- commands

def cmd_load(args):
    conflicts = []
    user_file = args.user_file or USER_FILE
    user = read_user(user_file, conflicts)
    result, mc = merge(user, "saved")
    conflicts += mc
    for c in conflicts:
        if c["kind"] == "enforce":
            warn("sonic-nix: saved %s = %s overridden by Nix: %s" % (label(c), c["current"], c["nix"]))
        else:
            warn("sonic-nix: saved config: " + describe(c))
    notes = []
    if not args.no_validate:
        problems = yang_validate(result)
        for p in problems:
            warn("sonic-nix: merged config: YANG: " + p)
        if problems and not problems[0].startswith("tables without YANG"):
            notes.append("merged config fails YANG validation (loaded anyway): " + problems[0][:200])
        elif problems:
            notes.append(problems[0])
    db = configdb()
    write_config(db, result, flush=True)
    atomic_write(os.path.join(RUN, "applied.json"), json.dumps(dict(base=BASE, modes=MODES), sort_keys=True))
    atomic_write(os.path.join(RUN, "boot-conflicts.json"), json.dumps(dict(conflicts=conflicts, notes=notes)))
    pending, _, _ = current_conflicts(db)
    publish(pending, notes)
    info("sonic-nix: CONFIG_DB loaded: %d tables, %d keys; %d conflict(s) with Nix; user layer %s"
         % (len(result), sum(len(e) for e in result.values()), len(conflicts), user_file if user else "(none)"))


def systemctl(*a):
    subprocess.run(["systemctl"] + list(a), check=True)


def stop_target():
    # "systemctl stop sonic.target" returns once the target's own stop job is done: a target is ordered after
    # the units it wants, so their (PartOf) stop jobs run after it, still queued, and the start below would
    # replace them: only the units that stopped fast restart around the CONFIG_DB flush (vrfmgrd and
    # intfmgrd race the VRFs; tests/vm-reload.nix). Named on the command line, systemctl waits for them all.
    units = subprocess.run(["systemctl", "show", "-p", "ConsistsOf", "--value", "sonic.target"],
                           check=True, capture_output=True, text=True).stdout.split()
    systemctl("stop", "sonic.target", *units)


def cmd_reload(args):
    # syncd's restartOnStop helper leaves the units alone while this exists.
    flag = os.path.join(RUN, "reloading")
    atomic_write(flag, "%d\n" % os.getpid())
    try:
        if not args.no_service_restart:
            stop_target()
        cmd_load(args)
        if not args.no_service_restart:
            systemctl("start", "sonic.target")
    finally:
        os.unlink(flag)


def cmd_apply(args):
    try:
        with open(os.path.join(RUN, "applied.json")) as f:
            old = json.load(f)
    except Exception:
        warn("sonic-nix apply: no applied.json (CONFIG_DB not loaded by sonic-nix yet); nothing to do")
        return
    db = configdb()
    live = live_config(db)
    obase = old["base"]
    changed = 0
    for table, entries in BASE.items():
        for key, fields in entries.items():
            for f, v in fields.items():
                mode = MODES[table][key][f]
                lv = live.get(table, {}).get(key, {}).get(f)
                ov = obase.get(table, {}).get(key, {}).get(f)
                if mode == ENFORCE:
                    do = norm(lv) != norm(v)
                else:   # seed: a new Nix value replaces only what was Nix's
                    do = norm(ov) != norm(v) and (lv is None or norm(lv) == norm(ov))
                if do:
                    info("sonic-nix apply: %s|%s %s: %s -> %s (%s)" % (table, key, f, fmt(lv), fmt(v), mode))
                    set_field(db, table, key, f, v)
                    changed += 1
    notes = []
    for table in META["ownedKeyTables"]:
        if set(obase.get(table, {})) != set(BASE.get(table, {})) or any(
                obase.get(table, {}).get(k, {}).get("lanes") != e.get("lanes") for k, e in BASE.get(table, {}).items()):
            notes.append("%s set changed (breakout): run 'sonic-nix reload' (restarts the switch stack)" % table)
            warn("sonic-nix apply: " + notes[-1])
    atomic_write(os.path.join(RUN, "applied.json"), json.dumps(dict(base=BASE, modes=MODES), sort_keys=True))
    b = boot_conflicts()
    b["notes"] = [n for n in b.get("notes", []) if "breakout" not in n] + notes
    atomic_write(os.path.join(RUN, "boot-conflicts.json"), json.dumps(b))
    pending, _, bnotes = current_conflicts(db)
    publish(pending, bnotes)
    info("sonic-nix apply: %d field(s) written" % changed)


def cmd_diff(args):
    db = configdb(wait=5)
    pending, live, notes = current_conflicts(db)
    saved_conf = []
    saved = read_user(USER_FILE, saved_conf)
    if args.json:
        rows = []
        for table, entries in BASE.items():
            for key, fields in entries.items():
                for f, v in fields.items():
                    rows.append(dict(table=table, key=key, field=f, mode=MODES[table][key][f], nix=fmt(v),
                                     running=fmt(live.get(table, {}).get(key, {}).get(f)),
                                     saved=fmt(saved.get(table, {}).get(key, {}).get(f)) if saved else None))
        json.dump(dict(conflicts=pending, notes=notes, fields=rows if args.all else
                       [r for r in rows if r["nix"] != r["running"] or (r["saved"] not in (None, "(absent)", r["nix"]))]),
                  sys.stdout, indent=2, sort_keys=True)
        print()
        return 1 if pending else 0
    print("Nix base: %s" % os.environ["SONIC_NIX_BASE"])
    print("saved:    %s%s" % (USER_FILE, "" if saved else " (none)"))
    if pending:
        print("\nConflicts (Nix wins at the next boot / nixos-rebuild switch):")
        w = max(len(label(c)) for c in pending)
        for c in pending:
            print("  %-*s  nix=%s  current=%s  [%s, %s]" % (w, label(c), c["nix"], c["current"], c["kind"], c["source"]))
    else:
        print("\nNo conflicts with enforced Nix settings.")
    seeded = []
    for table, entries in BASE.items():
        for key, fields in entries.items():
            for f, v in fields.items():
                lv = live.get(table, {}).get(key, {}).get(f)
                if MODES[table][key][f] == SEED and lv is not None and norm(lv) != norm(v):
                    seeded.append("  %s|%s %s: nix seed %s, running %s (user value sticks%s)" % (
                        table, key, f, fmt(v), fmt(lv),
                        "" if saved.get(table, {}).get(key, {}).get(f) is not None else "; not saved yet"))
    if seeded:
        print("\nSeeded fields changed by the user (kept):")
        print("\n".join(seeded))
    for n in notes:
        print("note: " + n)
    if args.all:
        print("\nAll Nix-managed fields:")
        for table, entries in BASE.items():
            for key, fields in entries.items():
                for f, v in fields.items():
                    print("  %s|%s %s = %s (%s)" % (table, key, f, fmt(v), MODES[table][key][f]))
    return 1 if pending else 0


def cmd_watch(args):
    from swsscommon import swsscommon
    db = configdb()
    last = None

    def refresh():
        nonlocal last
        pending, _, notes = current_conflicts(db)
        sig = json.dumps(pending, sort_keys=True)
        if sig != last:
            seen = {json.dumps(c, sort_keys=True) for c in json.loads(last or "[]")}
            for c in pending:
                if json.dumps(c, sort_keys=True) not in seen and "runtime" in c["source"]:
                    warn("sonic-nix: %s changed to %s, Nix %s says %s: kept for now, Nix wins at the next boot / rebuild"
                         % (label(c), c["current"], c["kind"], c["nix"]))
            publish(pending, notes)
            last = sig

    refresh()
    conn = swsscommon.DBConnector("CONFIG_DB", 0)
    sub = swsscommon.PubSub(conn)
    sub.psubscribe("__keyspace@%d__:*" % conn.getDbId())
    while True:
        got = sub.get_message(timeout=60)
        if got:
            time.sleep(0.5)       # coalesce a burst (config load, port setup)
            while sub.get_message(timeout=0.05):
                pass
        refresh()


def main():
    syslog.openlog("sonic-nix")
    ap = argparse.ArgumentParser(prog="sonic-nix", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sp = ap.add_subparsers(dest="cmd", required=True)
    p = sp.add_parser("load"); p.add_argument("--user-file"); p.add_argument("--no-validate", action="store_true")
    p = sp.add_parser("reload"); p.add_argument("--user-file"); p.add_argument("--no-service-restart", action="store_true")
    p.add_argument("--no-validate", action="store_true")
    sp.add_parser("apply")
    p = sp.add_parser("diff"); p.add_argument("--json", action="store_true"); p.add_argument("--all", action="store_true")
    sp.add_parser("watch")
    a = ap.parse_args()
    return {"load": cmd_load, "reload": cmd_reload, "apply": cmd_apply, "diff": cmd_diff, "watch": cmd_watch}[a.cmd](a) or 0


if __name__ == "__main__":
    sys.exit(main())
