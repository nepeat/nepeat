#!/usr/bin/env python3
"""Configure test channels on the Blonder Tongue EQAM-420B (dev unit).

Reads the unit's own forms (Main > Status > Edit = statusedit.cgi and Stream
Config = streamconfig.cgi), re-posts every field with its current value and
changes only the slots listed in PLAN. Prints the changes first; --apply posts
them and re-reads the pages to verify.

Unicast input semantics (manual + this unit): a slot's "Stream" entry is
UDP://<sender ip>:<port>; the sender transmits to the GigE data port.

usage: eqam_channels.py [--apply] [--host 10.158.117.10] [--user Admin --password pass]
"""
import argparse, html.parser, http.cookiejar, re, sys, urllib.parse, urllib.request

SENDER = "10.60.2.208"
# slot: (channel, short name, port)
PLAN = {
    5:  (900, "TESTCAT", 5900),
    6:  (901, "TESTSAX", 5901),
    9:  (902, "TESTNYA", 5902),
    10: (903, "TSTCRAB", 5903),
    13: (904, "TSTCARA", 5904),
    14: (905, "TSTFISH", 5905),
    17: (906, "TESTHD", 5906),      # RF 54: MPEG-2 1080i woven from the 60p original
    18: (907, "TEST264", 5907),     # RF 54: H.264 1080p29.97 (TV H.264-over-QAM test)
    21: (908, "TEST265", 5908),     # RF 55 alone: HEVC acceptance test
}
# slots to empty (the 2026-10-16 H.264 A/B variants 908-912 on 19, 21-24)
CLEAR = [19, 22, 23, 24]


class Form(html.parser.HTMLParser):
    """collect name -> value of inputs, selects (selected option) of a page"""
    def __init__(self):
        super().__init__()
        self.fields, self.order = {}, []
        self._select, self._first = None, None

    def handle_starttag(self, tag, a):
        a = dict(a)
        if tag == "input" and a.get("name") and a.get("type", "text") not in ("submit", "button"):
            self._set(a["name"], a.get("value", ""))
        elif tag == "select":
            self._select, self._first = a.get("name"), None
        elif tag == "option" and self._select:
            v = a.get("value", "")
            if self._first is None:
                self._first = v
                self._set(self._select, v)
            if "selected" in a:
                self._set(self._select, v)

    def handle_endtag(self, tag):
        if tag == "select":
            self._select = None

    def _set(self, k, v):
        if k not in self.fields:
            self.order.append(k)
        self.fields[k] = v


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true")
    ap.add_argument("--host", default="10.158.117.10")
    ap.add_argument("--user", default="Admin")
    ap.add_argument("--password", default="pass")
    o = ap.parse_args()
    base = f"http://{o.host}/cgi-bin"
    op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    get = lambda u: op.open(u, timeout=20).read().decode("latin-1")
    post = lambda u, d: op.open(u, urllib.parse.urlencode(d).encode(), timeout=30).read().decode("latin-1")

    page = get(f"{base}/login.cgi")
    sid = re.search(r'name="session_id" value="(\d+)"', page)
    r = post(f"{base}/login.cgi", {"txtUserName": o.user, "txtPassword": o.password,
                                   "session_id": sid.group(1) if sid else "", "btnSubmit": "Submit"})
    m = re.search(r"session_id=(\d+)", r)
    if not m:
        sys.exit("login failed")
    s = m.group(1)
    op.addheaders = [("Cookie", f"session_id={s}")]

    def read(pg):
        f = Form()
        f.feed(get(f"{base}/{pg}.cgi?session_id={s}"))
        f.fields["session_id"] = s
        return f

    def want(pg):
        w = {}
        for slot, (ch, name, port) in PLAN.items():
            if pg == "statusedit":
                w[f"streamGroupAddress{slot}"] = f"UDP://{SENDER}:{port}"
                w[f"streamName{slot}"] = name
            else:
                w[f"programNumber{slot}"] = str(ch)
                w[f"shortName{slot}"] = name
                w[f"majorNumber{slot}"] = str(ch)
                w[f"minorNumber{slot}"] = "1"
        for slot in CLEAR:
            if pg == "statusedit":
                w[f"streamGroupAddress{slot}"] = w[f"streamName{slot}"] = ""
            else:
                w[f"shortName{slot}"] = ""
                for k in ("programNumber", "majorNumber", "minorNumber"):
                    w[f"{k}{slot}"] = "0"
        return w

    ok = True
    try:
        for pg in ("statusedit", "streamconfig"):
            f = read(pg)
            w = want(pg)
            missing = [k for k in w if k not in f.fields]
            if missing:
                sys.exit(f"{pg}: fields not found: {missing}")
            changes = {k: (f.fields[k], v) for k, v in w.items() if f.fields[k] != v}
            print(f"{pg}: {len(f.fields)} fields, {len(changes)} to change")
            for k, (a, b) in changes.items():
                print(f"   {k}: {a!r} -> {b!r}")
            if o.apply and changes:
                data = [(k, w.get(k, f.fields[k])) for k in f.order]
                data.append(("btnSave", "Save"))
                post(f"{base}/{pg}.cgi", data)
                after = read(pg)
                bad = {k: after.fields.get(k) for k, v in w.items() if after.fields.get(k) != v}
                # nothing else may have moved
                moved = {k: (f.fields[k], after.fields.get(k)) for k in f.fields
                         if k not in w and k != "session_id" and after.fields.get(k) != f.fields[k]}
                print(f"   verify: {'OK' if not bad and not moved else 'MISMATCH'}"
                      + (f" not applied {bad}" if bad else "") + (f" other fields moved {moved}" if moved else ""))
                ok &= not bad and not moved
    finally:
        get(f"{base}/status.cgi?logout=1&session_id={s}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
