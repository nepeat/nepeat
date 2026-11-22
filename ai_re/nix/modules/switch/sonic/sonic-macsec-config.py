"""MACsec profiles and port assignments from the NixOS configuration -> CONFIG_DB.

  sonic-macsec-config.py DECLARED.json
DECLARED: {"profiles": {name: {"cakFile": path, "cakHexDigits": 32|64, "fields": {...}}}, "ports": {port: profile},
           "stateFile": path}
The CAK is read from cakFile (hex, one line) and stored as SONiC stores it: MACSEC_PROFILE primary_cak in the
"type 7" encoding that macsecmgrd decodes. What an earlier run wrote and is no longer declared is removed (state
file); entries this never wrote are not touched. The key is never printed. Exit 1 when a key file is missing or
malformed: nothing of that profile is written and no port is assigned to it.
"""
import json, os, re, sys
from swsscommon.swsscommon import ConfigDBConnector

SALT = b"dsfd;kfoA,.iyewrkldJKDHSUBsgvca69834ncxv9873254k;fg87"


def type7(text, index=7):
    """Cisco type 7: two decimal digits (start index), then each character XOR the constant, as hex."""
    return "%02d" % index + "".join("%02x" % (ord(c) ^ SALT[(index + i) % len(SALT)]) for i, c in enumerate(text))


def read_cak(path, digits):
    st = os.stat(path)
    if st.st_mode & 0o077:
        raise ValueError("%s is readable by group or others (chmod 600)" % path)
    cak = open(path).read().strip()
    if not re.fullmatch("[0-9a-fA-F]{%d}" % digits, cak):
        raise ValueError("%s: expected %d hex digits on one line" % (path, digits))
    return cak.lower()


def main():
    d = json.load(open(sys.argv[1]))
    try:
        old = json.load(open(d["stateFile"]))
    except (OSError, ValueError):
        old = {"profiles": [], "ports": {}}
    db = ConfigDBConnector()
    db.connect()
    rc = 0
    good = {}
    for name, p in sorted(d["profiles"].items()):
        try:
            cak = read_cak(p["cakFile"], p["cakHexDigits"])
        except (OSError, ValueError) as e:
            print("MACsec profile %s: %s: not written" % (name, e), file=sys.stderr)
            rc = 1
            continue
        good[name] = dict(p["fields"], primary_cak=type7(cak))
    ports = db.get_table("PORT")
    # 1. ports that leave a profile (ours, no longer declared or its profile is unusable)
    for port, prof in sorted(old.get("ports", {}).items()):
        want = d["ports"].get(port)
        cur = ports.get(port, {}).get("macsec")
        if cur == prof and (want != prof or want not in good):
            e = dict(ports[port])
            del e["macsec"]
            db.set_entry("PORT", port, e)
            print("port %s: MACsec profile %s removed" % (port, prof))
    # 2. profiles: write the declared ones, remove ours that are gone
    for name, fields in good.items():
        if db.get_entry("MACSEC_PROFILE", name) != fields:
            db.set_entry("MACSEC_PROFILE", name, fields)
            print("MACsec profile %s written (%s, key from %s)" % (name, fields["cipher_suite"], d["profiles"][name]["cakFile"]))
    for name in old.get("profiles", []):
        if name not in d["profiles"] and db.get_entry("MACSEC_PROFILE", name):
            db.set_entry("MACSEC_PROFILE", name, None)
            print("MACsec profile %s removed" % name)
    # 3. ports onto their profile
    done = {}
    for port, prof in sorted(d["ports"].items()):
        if prof not in good:
            continue
        if port not in ports:
            print("port %s: not in CONFIG_DB PORT" % port, file=sys.stderr)
            rc = 1
            continue
        if ports[port].get("macsec") != prof:
            db.mod_entry("PORT", port, {"macsec": prof})
            print("port %s: MACsec profile %s" % (port, prof))
        done[port] = prof
    os.makedirs(os.path.dirname(d["stateFile"]), exist_ok=True)
    with open(d["stateFile"] + ".tmp", "w") as f:
        json.dump({"profiles": sorted(good), "ports": done}, f)
    os.replace(d["stateFile"] + ".tmp", d["stateFile"])
    return rc


if __name__ == "__main__":
    sys.exit(main())
