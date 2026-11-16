#!/usr/bin/env python3
"""SONiC switch state for the node exporter textfile collector (sonic_*).

Reads redis directly (no SONiC CLI: "show" takes seconds on an e6500) and
prints Prometheus text format on stdout; the textfile producer wrapper
(nix/modules/monitoring) publishes it atomically. Python stdlib only: a
small RESP client, KEYS + pipelined HGETALL per table, route counts by a
Lua script inside redis.

  sonic-metrics                               the running switch
  sonic-metrics --socket PATH --db-config F   another redis / DB map
  sonic-metrics --conflicts F --systemctl P   test hooks

Port labels carry the SONiC name (port="Ethernet0") and the front-panel
alias (alias="Ethernet1/1"). Without redis the run still succeeds and says
sonic_up 0 (plus the redis-free families: conflicts, services). A table
that fails to read gives sonic_collector_success{collector} 0, the others
are still printed.
"""
import argparse
import json
import os
import socket
import subprocess
import sys
import time

DB_CONFIG = "/var/run/redis/sonic-db/database_config.json"
SOCKET = "/var/run/redis/redis.sock"
CONFLICTS = "/run/sonic-nix/conflicts.json"
DEFAULT_DBS = {"APPL_DB": (0, ":"), "ASIC_DB": (1, ":"), "COUNTERS_DB": (2, ":"),
               "CONFIG_DB": (4, "|"), "STATE_DB": (6, "|")}
UNITS = ["sonic-*.service", "redis-sonic.service"]

# RFC 5880 (BFD) and RFC 4273 bgpPeerState numbering.
BFD_STATES = {"admin_down": 0, "down": 1, "init": 2, "up": 3}
BGP_STATES = {"idle": 1, "connect": 2, "active": 3, "opensent": 4, "openconfirm": 5, "established": 6}

PORT_COUNTERS = [  # (family, help, SAI stats summed)
    ("rx_bytes", "Octets received", ["SAI_PORT_STAT_IF_IN_OCTETS"]),
    ("rx_packets", "Packets received (unicast + non-unicast)", ["SAI_PORT_STAT_IF_IN_UCAST_PKTS", "SAI_PORT_STAT_IF_IN_NON_UCAST_PKTS"]),
    ("rx_errors", "Receive errors", ["SAI_PORT_STAT_IF_IN_ERRORS"]),
    ("rx_discards", "Received packets discarded", ["SAI_PORT_STAT_IF_IN_DISCARDS"]),
    ("tx_bytes", "Octets sent", ["SAI_PORT_STAT_IF_OUT_OCTETS"]),
    ("tx_packets", "Packets sent (unicast + non-unicast)", ["SAI_PORT_STAT_IF_OUT_UCAST_PKTS", "SAI_PORT_STAT_IF_OUT_NON_UCAST_PKTS"]),
    ("tx_errors", "Transmit errors", ["SAI_PORT_STAT_IF_OUT_ERRORS"]),
    ("tx_discards", "Packets discarded on transmit", ["SAI_PORT_STAT_IF_OUT_DISCARDS"]),
]

# APPL_DB ROUTE_TABLE:[<vrf>:]<prefix> -> {"<vrf>|<family>": n}; VRF names start with "Vrf".
LUA_APPL_ROUTES = """
local c = {}
for _, k in ipairs(redis.call('KEYS', ARGV[1] .. '*')) do
  local rest = string.sub(k, #ARGV[1] + 1)
  local vrf = 'default'
  if string.sub(rest, 1, 3) == 'Vrf' then
    local i = string.find(rest, ':', 1, true)
    if i then vrf = string.sub(rest, 1, i - 1); rest = string.sub(rest, i + 1) end
  end
  local fam = 'ipv4'
  if string.find(rest, ':', 1, true) then fam = 'ipv6' end
  local key = vrf .. '|' .. fam
  c[key] = (c[key] or 0) + 1
end
local out = {}
for k, v in pairs(c) do out[#out + 1] = k; out[#out + 1] = v end
return out
"""
# ASIC_DB route entries by family of their "dest".
LUA_ASIC_ROUTES = """
local c = {ipv4 = 0, ipv6 = 0}
for _, k in ipairs(redis.call('KEYS', ARGV[1])) do
  local d = string.match(k, '"dest":"([^"]*)"') or ''
  if string.find(d, ':', 1, true) then c.ipv6 = c.ipv6 + 1 else c.ipv4 = c.ipv4 + 1 end
end
return {'ipv4', c.ipv4, 'ipv6', c.ipv6}
"""


def esc(v):
    return str(v).replace("\\", "\\\\").replace("\n", "\\n").replace('"', '\\"')


def num(v):
    """A redis value as a float, None for '', N/A and other non-numbers."""
    try:
        f = float(v)
    except (TypeError, ValueError):
        return None
    return f if f == f else None


class Out:
    def __init__(self):
        self.fams = {}

    def add(self, name, help_, value, labels=None, typ="gauge"):
        if value is None:
            return
        f = self.fams.setdefault(name, [typ, help_, []])
        lab = ""
        if labels:
            lab = "{" + ",".join('%s="%s"' % (k, esc(v)) for k, v in labels.items()) + "}"
        if isinstance(value, bool):
            value = int(value)
        # %.15g: the cross-built target Python lacks short float repr.
        f[2].append("%s%s %s" % (name, lab, ("%.15g" % value) if isinstance(value, float) else value))

    def render(self):
        lines = []
        for name, (typ, help_, samples) in self.fams.items():
            lines += ["# HELP %s %s" % (name, help_), "# TYPE %s %s" % (name, typ)] + samples
        return "\n".join(lines) + "\n"


class RedisError(Exception):
    pass


class Redis:
    """Minimal RESP2 client: pipelined commands, replies as str/int/list/None/RedisError."""

    def __init__(self, path=None, host=None, port=None, timeout=3.0):
        if path:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            s.settimeout(timeout)
            s.connect(path)
        else:
            s = socket.create_connection((host, port), timeout)
        self.sock = s
        self.buf = bytearray()
        self.pos = 0

    def close(self):
        self.sock.close()

    def pipeline(self, cmds, chunk=1000):
        replies = []
        for i in range(0, len(cmds), chunk):
            part = cmds[i:i + chunk]
            out = bytearray()
            for c in part:
                out += b"*%d\r\n" % len(c)
                for a in c:
                    a = a if isinstance(a, bytes) else str(a).encode()
                    out += b"$%d\r\n%s\r\n" % (len(a), a)
            self.sock.sendall(out)
            replies += [self._reply() for _ in part]
        return replies

    def _fill(self):
        if self.pos:
            del self.buf[:self.pos]
            self.pos = 0
        d = self.sock.recv(262144)
        if not d:
            raise ConnectionError("redis closed the connection")
        self.buf += d

    def _line(self):
        while True:
            i = self.buf.find(b"\r\n", self.pos)
            if i >= 0:
                line = bytes(self.buf[self.pos:i])
                self.pos = i + 2
                return line
            self._fill()

    def _reply(self):
        line = self._line()
        t, rest = line[:1], line[1:]
        if t == b"+":
            return rest.decode()
        if t == b"-":
            return RedisError(rest.decode(errors="replace"))
        if t == b":":
            return int(rest)
        if t == b"$":
            n = int(rest)
            if n < 0:
                return None
            while len(self.buf) - self.pos < n + 2:
                self._fill()
            v = bytes(self.buf[self.pos:self.pos + n])
            self.pos += n + 2
            return v.decode(errors="replace")
        if t == b"*":
            n = int(rest)
            return None if n < 0 else [self._reply() for _ in range(n)]
        raise RedisError("unexpected reply %r" % line[:40])


def ok(v):
    if isinstance(v, RedisError):
        raise v
    return v


class Sonic:
    """Cached table reads: hashes(db, table) -> {key without table prefix: fields}."""

    def __init__(self, r, dbs):
        self.r, self.dbs, self.cache = r, dbs, {}

    def cmds(self, db, cmds):
        return self.r.pipeline([["SELECT", self.dbs[db][0]]] + cmds)[1:]

    def hashes(self, db, table):
        if (db, table) in self.cache:
            return self.cache[(db, table)]
        sep = self.dbs[db][1]
        prefix = table + sep
        keys = sorted(ok(self.cmds(db, [["KEYS", prefix + "*"]])[0]))
        out = {}
        for k, v in zip(keys, self.cmds(db, [["HGETALL", k] for k in keys])):
            if isinstance(v, list) and v:
                out[k[len(prefix):]] = dict(zip(v[0::2], v[1::2]))
        self.cache[(db, table)] = out
        return out

    def hgetall(self, db, key):
        v = ok(self.cmds(db, [["HGETALL", key]])[0])
        return dict(zip(v[0::2], v[1::2])) if v else {}

    def hmget(self, db, keys, fields):
        """[{field: value}] for each key (None values dropped)."""
        rep = self.cmds(db, [["HMGET", k] + fields for k in keys])
        return [dict((f, x) for f, x in zip(fields, v) if x is not None) if isinstance(v, list) else {} for v in rep]

    def exists(self, db, key):
        return ok(self.cmds(db, [["EXISTS", key]])[0]) == 1

    def eval(self, db, script, *args):
        return ok(self.cmds(db, [["EVAL", script, 0] + list(args)])[0])


def load_dbs(path):
    """{name: (id, separator)} and (socket, host, port) from SONiC's database_config.json."""
    dbs, conn = dict(DEFAULT_DBS), (SOCKET, "127.0.0.1", 6379)
    try:
        with open(path) as f:
            cfg = json.load(f)
        for name, d in cfg.get("DATABASES", {}).items():
            if name in dbs:
                dbs[name] = (int(d["id"]), d.get("separator", dbs[name][1]))
        inst = cfg.get("INSTANCES", {}).get("redis", {})
        conn = (inst.get("unix_socket_path") or SOCKET, inst.get("hostname", "127.0.0.1"), int(inst.get("port", 6379)))
    except (OSError, ValueError, KeyError, TypeError):
        pass
    return dbs, conn


def connect(sock_path, host, port, timeout):
    errors = []
    for kw in ([dict(path=sock_path)] if sock_path else []) + ([dict(host=host, port=port)] if host else []):
        try:
            r = Redis(timeout=timeout, **kw)
            if r.pipeline([["PING"]])[0] == "PONG":
                return r, None
            r.close()
        except OSError as e:
            errors.append(str(e))
    return None, "; ".join(errors) or "no redis address"


# ------------------------------------------------------------------ collectors

def collect_device(s, o, ctx):
    md = s.hashes("CONFIG_DB", "DEVICE_METADATA").get("localhost", {})
    o.add("sonic_device_info", "SONiC device metadata (CONFIG_DB DEVICE_METADATA|localhost).", 1,
          {"platform": md.get("platform", ""), "hwsku": md.get("hwsku", ""), "type": md.get("type", ""),
           "bgp_asn": md.get("bgp_asn", "")})
    o.add("sonic_config_db_initialized", "1 when CONFIG_DB_INITIALIZED is set (config loaded).",
          int(s.exists("CONFIG_DB", "CONFIG_DB_INITIALIZED")))
    o.add("sonic_port_init_done", "1 when orchagent created all ports (APPL_DB PORT_TABLE:PortInitDone).",
          int(s.exists("APPL_DB", "PORT_TABLE" + s.dbs["APPL_DB"][1] + "PortInitDone")))


def port_order(item):
    name, f = item
    digits = "".join(ch for ch in name if ch.isdigit())
    return (int(digits) if digits else 1 << 30, name)


def collect_ports(s, o, ctx):
    cfg = s.hashes("CONFIG_DB", "PORT")
    appl = s.hashes("APPL_DB", "PORT_TABLE")
    for name, c in sorted(cfg.items(), key=port_order):
        a = appl.get(name, {})
        lab = {"port": name, "alias": c.get("alias", name)}
        o.add("sonic_port_info", "Front-panel port (CONFIG_DB PORT): SONiC name, alias, cage index, lanes.", 1,
              dict(lab, index=c.get("index", ""), lanes=c.get("lanes", "")))
        o.add("sonic_port_admin_up", "1 when the port is admin up (APPL_DB PORT_TABLE, else CONFIG_DB).",
              int((a.get("admin_status") or c.get("admin_status")) == "up"), lab)
        o.add("sonic_port_oper_up", "1 when the port is oper up (APPL_DB PORT_TABLE oper_status).",
              int(a.get("oper_status") == "up"), lab)
        sp = num(a.get("speed") or c.get("speed"))
        o.add("sonic_port_speed_bytes", "Port speed in bytes per second (as node_network_speed_bytes).",
              None if sp is None else sp * 1e6 / 8, lab)
        o.add("sonic_port_mtu_bytes", "Port MTU.", num(a.get("mtu") or c.get("mtu")), lab)


def collect_counters(s, o, ctx):
    names = s.hgetall("COUNTERS_DB", "COUNTERS_PORT_NAME_MAP")
    aliases = ctx["alias"]
    ports = sorted(names.items(), key=port_order)
    fields = sorted({f for _, _, fs in PORT_COUNTERS for f in fs})
    sep = s.dbs["COUNTERS_DB"][1]
    rows = s.hmget("COUNTERS_DB", ["COUNTERS" + sep + oid for _, oid in ports], fields)
    for (name, _), row in zip(ports, rows):
        lab = {"port": name, "alias": aliases.get(name, name)}
        for fam, help_, fs in PORT_COUNTERS:
            vals = [num(row.get(f)) for f in fs if row.get(f) is not None]
            vals = [v for v in vals if v is not None]
            if vals:
                o.add("sonic_port_%s_total" % fam, help_ + " (COUNTERS_DB, flex counter PORT).", int(sum(vals)), lab, "counter")


def collect_portchannels(s, o, ctx):
    cfg = s.hashes("CONFIG_DB", "PORTCHANNEL")
    members = s.hashes("CONFIG_DB", "PORTCHANNEL_MEMBER")
    appl = s.hashes("APPL_DB", "LAG_TABLE")
    appl_m = s.hashes("APPL_DB", "LAG_MEMBER_TABLE")
    state_m = s.hashes("STATE_DB", "LAG_MEMBER_TABLE")
    asep, ssep = s.dbs["APPL_DB"][1], s.dbs["STATE_DB"][1]
    by_pc = {}
    for k in members:
        pc, _, port = k.partition("|")
        by_pc.setdefault(pc, []).append(port)
    for pc in sorted(set(cfg) | set(by_pc)):
        c, a = cfg.get(pc, {}), appl.get(pc, {})
        lab = {"portchannel": pc}
        o.add("sonic_portchannel_admin_up", "1 when the PortChannel is admin up.",
              int((a.get("admin_status") or c.get("admin_status", "up")) == "up"), lab)
        o.add("sonic_portchannel_oper_up", "1 when the PortChannel is oper up (APPL_DB LAG_TABLE, teamsyncd).",
              int(a.get("oper_status") == "up"), lab)
        o.add("sonic_portchannel_min_links", "Configured min_links.", num(c.get("min_links")), lab)
        ports = sorted(by_pc.get(pc, []), key=lambda p: port_order((p, None)))
        sel = 0
        for port in ports:
            ml = dict(lab, port=port, alias=ctx["alias"].get(port, port))
            st = state_m.get(pc + ssep + port, {})
            selected = st.get("runner.aggregator.selected", st.get("runner.selected")) == "true"
            sel += selected
            o.add("sonic_portchannel_member_selected", "1 when LACP selected the member (STATE_DB LAG_MEMBER_TABLE, tlm_teamd).",
                  int(selected), ml)
            o.add("sonic_portchannel_member_enabled", "1 when teamd enabled the member for traffic (APPL_DB LAG_MEMBER_TABLE status).",
                  int(appl_m.get(pc + asep + port, {}).get("status") == "enabled"), ml)
        o.add("sonic_portchannel_members", "Members configured (CONFIG_DB PORTCHANNEL_MEMBER).", len(ports), lab)
        o.add("sonic_portchannel_members_selected", "Members selected by LACP.", sel, lab)


def collect_bfd(s, o, ctx):
    for key, f in sorted(s.hashes("STATE_DB", "BFD_SESSION_TABLE").items()):
        parts = key.split("|")
        if len(parts) < 3:
            continue
        vrf, ifname, peer = parts[0], parts[1], "|".join(parts[2:])
        lab = {"vrf": vrf, "interface": ifname, "alias": ctx["alias"].get(ifname, ""), "peer": peer}
        st = f.get("state", "").lower()
        o.add("sonic_bfd_session_info", "BFD session (STATE_DB BFD_SESSION_TABLE, bfdsyncd): local address, type, multihop.", 1,
              dict(lab, local_addr=f.get("local_addr", ""), type=f.get("type", ""), multihop=f.get("multihop", "")))
        o.add("sonic_bfd_session_state", "BFD session state, RFC 5880 numbering: 0 AdminDown, 1 Down, 2 Init, 3 Up.",
              BFD_STATES.get(st), lab)
        o.add("sonic_bfd_session_up", "1 when the BFD session is Up.", int(st == "up"), lab)
        for fld, fam, help_ in (("tx_interval", "tx_interval_seconds", "Desired transmit interval."),
                                ("rx_interval", "rx_interval_seconds", "Required receive interval.")):
            v = num(f.get(fld))
            o.add("sonic_bfd_session_" + fam, help_, None if v is None else v / 1000.0, lab)
        o.add("sonic_bfd_session_detect_multiplier", "Detection multiplier.", num(f.get("multiplier")), lab)


def bgp_code(state):
    st = state.split("(")[0].strip().lower()
    return BGP_STATES.get(st, 0)


def collect_bgp(s, o, ctx):
    conf = {}
    for key, f in s.hashes("CONFIG_DB", "BGP_NEIGHBOR").items():
        vrf, _, ip = key.rpartition("|")
        conf[(vrf or "default", ip)] = f
    for (vrf, ip), f in sorted(conf.items()):
        lab = {"vrf": vrf, "neighbor": ip, "name": f.get("name", "")}
        o.add("sonic_bgp_neighbor_info", "BGP neighbor configured in CONFIG_DB BGP_NEIGHBOR.", 1,
              dict(lab, asn=f.get("asn", ""), local_addr=f.get("local_addr", ""), bfd=f.get("bfd", "false")))
        o.add("sonic_bgp_neighbor_admin_up", "1 unless the neighbor's admin_status is down.",
              int(f.get("admin_status", "up") != "down"), lab)
    # bgpmon: "show bgp summary json" of the default VRF only.
    for ip, f in sorted(s.hashes("STATE_DB", "NEIGH_STATE_TABLE").items()):
        lab = {"vrf": "default", "neighbor": ip, "name": conf.get(("default", ip), {}).get("name", "")}
        state = f.get("state", "")
        o.add("sonic_bgp_neighbor_state", "BGP session state (STATE_DB NEIGH_STATE_TABLE, bgpmon, default VRF only), "
              "RFC 4273 numbering: 1 Idle, 2 Connect, 3 Active, 4 OpenSent, 5 OpenConfirm, 6 Established, 0 other.",
              bgp_code(state), lab)
        o.add("sonic_bgp_neighbor_established", "1 when the BGP session is Established.", int(bgp_code(state) == 6), lab)


def collect_lldp(s, o, ctx):
    ent = s.hashes("APPL_DB", "LLDP_ENTRY_TABLE")
    for name, alias in ctx["ports"]:
        o.add("sonic_lldp_neighbors", "LLDP neighbors seen on the port (APPL_DB LLDP_ENTRY_TABLE, lldp_syncd).",
              int(name in ent), {"port": name, "alias": alias})
    for name, f in sorted(ent.items(), key=port_order):
        o.add("sonic_lldp_neighbor_info", "LLDP neighbor of the port: remote system name, port ID and description, chassis ID.", 1,
              {"port": name, "alias": ctx["alias"].get(name, ""), "remote_system": f.get("lldp_rem_sys_name", ""),
               "remote_port": f.get("lldp_rem_port_id", ""), "remote_port_desc": f.get("lldp_rem_port_desc", ""),
               "remote_chassis": f.get("lldp_rem_chassis_id", "")})


def collect_transceivers(s, o, ctx):
    info = s.hashes("STATE_DB", "TRANSCEIVER_INFO")
    dom = s.hashes("STATE_DB", "TRANSCEIVER_DOM_SENSOR")
    temp = s.hashes("STATE_DB", "TRANSCEIVER_DOM_TEMPERATURE")
    for name, alias in ctx["ports"]:
        lab = {"port": name, "alias": alias}
        o.add("sonic_transceiver_present", "1 when xcvrd sees a module (STATE_DB TRANSCEIVER_INFO).", int(name in info), lab)
        if name not in info:
            continue
        i = info[name]
        # xcvrd keeps the EEPROM's space padding ("FAKE-VENDOR     ").
        o.add("sonic_transceiver_info", "Module identity from xcvrd: type, vendor, model.", 1,
              dict(lab, type=i.get("type", "").strip(), vendor=i.get("manufacturer", "").strip(), model=i.get("model", "").strip()))
        d = dom.get(name, {})
        t = num(d.get("temperature"))
        if t is None:
            t = num(temp.get(name, {}).get("temperature"))
        o.add("sonic_transceiver_temperature_celsius", "Module temperature (STATE_DB TRANSCEIVER_DOM_SENSOR).", t, lab)
        o.add("sonic_transceiver_voltage_volts", "Module supply voltage (STATE_DB TRANSCEIVER_DOM_SENSOR).", num(d.get("voltage")), lab)


def collect_acl(s, o, ctx):
    rules = s.hashes("CONFIG_DB", "ACL_RULE")
    state = s.hashes("STATE_DB", "ACL_RULE_TABLE")
    for key, f in sorted(rules.items()):
        table, _, rule = key.partition("|")
        lab = {"table": table, "rule": rule}
        action = f.get("PACKET_ACTION") or f.get("REDIRECT_ACTION") or f.get("MIRROR_ACTION") or ""
        o.add("sonic_acl_rule_info", "ACL rule in CONFIG_DB ACL_RULE: action, priority.", 1,
              dict(lab, action=action, priority=f.get("PRIORITY", "")))
        o.add("sonic_acl_rule_active", "1 when orchagent reports the rule Active (STATE_DB ACL_RULE_TABLE).",
              int(state.get(key, {}).get("status") == "Active"), lab)
    # aclorch's map "<table>:<rule>" -> counter oid; values only while "counterpoll acl" is enabled.
    cmap = sorted(s.hgetall("COUNTERS_DB", "ACL_COUNTER_RULE_MAP").items())
    sep = s.dbs["COUNTERS_DB"][1]
    rows = s.hmget("COUNTERS_DB", ["COUNTERS" + sep + oid for _, oid in cmap],
                   ["SAI_ACL_COUNTER_ATTR_PACKETS", "SAI_ACL_COUNTER_ATTR_BYTES"])
    for (ident, _), row in zip(cmap, rows):
        table, _, rule = ident.partition(sep)
        lab = {"table": table, "rule": rule}
        p, b = num(row.get("SAI_ACL_COUNTER_ATTR_PACKETS")), num(row.get("SAI_ACL_COUNTER_ATTR_BYTES"))
        o.add("sonic_acl_rule_packets_total", "Packets matched by the ACL rule (COUNTERS_DB, flex counter ACL).",
              None if p is None else int(p), lab, "counter")
        o.add("sonic_acl_rule_bytes_total", "Bytes matched by the ACL rule.", None if b is None else int(b), lab, "counter")


def collect_copp(s, o, ctx):
    for group, f in sorted(s.hashes("APPL_DB", "COPP_TABLE").items()):
        lab = {"group": group}
        traps = [t.strip() for t in f.get("trap_ids", "").split(",") if t.strip()]
        o.add("sonic_copp_group_info", "CoPP trap group (APPL_DB COPP_TABLE, coppmgrd): action, queue, meter.", 1,
              dict(lab, trap_action=f.get("trap_action", ""), queue=f.get("queue", ""), meter_type=f.get("meter_type", ""),
                   mode=f.get("mode", "")))
        o.add("sonic_copp_group_cir", "Policer committed rate (packets or bytes per second, by meter_type).", num(f.get("cir")), lab)
        o.add("sonic_copp_group_cbs", "Policer committed burst.", num(f.get("cbs")), lab)
        o.add("sonic_copp_group_traps", "Traps in the group.", len(traps), lab)
        for t in traps:
            o.add("sonic_copp_trap_info", "Trap -> CoPP group.", 1, {"trap": t, "group": group})
    cmap = sorted(s.hgetall("COUNTERS_DB", "COUNTERS_TRAP_NAME_MAP").items())
    sep = s.dbs["COUNTERS_DB"][1]
    rows = s.hmget("COUNTERS_DB", ["COUNTERS" + sep + oid for _, oid in cmap],
                   ["SAI_COUNTER_STAT_PACKETS", "SAI_COUNTER_STAT_BYTES"])
    for (trap, _), row in zip(cmap, rows):
        p, b = num(row.get("SAI_COUNTER_STAT_PACKETS")), num(row.get("SAI_COUNTER_STAT_BYTES"))
        o.add("sonic_copp_trap_packets_total", "Packets trapped to the CPU (COUNTERS_DB, flex counter FLOW_CNT_TRAP).",
              None if p is None else int(p), {"trap": trap}, "counter")
        o.add("sonic_copp_trap_bytes_total", "Bytes trapped to the CPU.", None if b is None else int(b), {"trap": trap}, "counter")


def collect_routes(s, o, ctx):
    rep = s.eval("APPL_DB", LUA_APPL_ROUTES, "ROUTE_TABLE" + s.dbs["APPL_DB"][1])
    counts = {}
    for k, v in zip(rep[0::2], rep[1::2]):
        vrf, _, fam = k.rpartition("|")
        counts[(vrf, fam)] = v
    counts.setdefault(("default", "ipv4"), 0)
    for (vrf, fam), v in sorted(counts.items()):
        o.add("sonic_routes", "Routes in APPL_DB ROUTE_TABLE (fpmsyncd, orchagent's input) by VRF and family.", v,
              {"vrf": vrf, "family": fam})
    sep = s.dbs["ASIC_DB"][1]
    rep = s.eval("ASIC_DB", LUA_ASIC_ROUTES, "ASIC_STATE" + sep + "SAI_OBJECT_TYPE_ROUTE_ENTRY" + sep + "*")
    for fam, v in zip(rep[0::2], rep[1::2]):
        o.add("sonic_asic_routes", "Route entries in ASIC_DB (programmed through syncd) by family.", v, {"family": fam})


def collect_conflicts(o, path):
    with open(path) as f:
        st = json.load(f)
    conflicts = st.get("conflicts", [])
    o.add("sonic_nix_conflicts", "Pending CONFIG_DB conflicts with the Nix configuration (/run/sonic-nix/conflicts.json).", len(conflicts))
    by = {}
    for c in conflicts:
        by[c.get("table", "")] = by.get(c.get("table", ""), 0) + 1
    for t, n in sorted(by.items()):
        o.add("sonic_nix_conflicts_by_table", "Pending conflicts per CONFIG_DB table.", n, {"table": t})
    o.add("sonic_nix_notes", "Notes next to the conflicts (e.g. seeded fields the user changed).", len(st.get("notes", [])))
    o.add("sonic_nix_conflicts_updated_timestamp_seconds", "When sonic-nix last wrote the conflicts file.", num(st.get("time")))


def collect_services(o, systemctl):
    props = "Id,ActiveState,NRestarts,ActiveEnterTimestampMonotonic"
    res = subprocess.run([systemctl, "show", "--property=" + props, "--"] + UNITS, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, universal_newlines=True, timeout=10)
    if res.returncode != 0:
        raise RuntimeError("systemctl show: exit %d" % res.returncode)
    now_wall, now_mono = time.time(), time.monotonic()
    units = []
    for block in res.stdout.strip().split("\n\n"):
        u = dict(line.split("=", 1) for line in block.splitlines() if "=" in line)
        if u.get("Id"):
            units.append(u)
    for u in sorted(units, key=lambda u: u["Id"]):
        lab = {"unit": u["Id"]}
        o.add("sonic_service_active", "1 when the unit is active (sonic-*.service, redis-sonic.service).",
              int(u.get("ActiveState") == "active"), lab)
        o.add("sonic_service_restarts", "Automatic restarts (systemd NRestarts) since the unit was last started by hand.",
              num(u.get("NRestarts")), lab)
        mono = num(u.get("ActiveEnterTimestampMonotonic"))
        if mono:
            o.add("sonic_service_start_timestamp_seconds", "When the unit last became active (Unix time); "
                  "changes() counts every start, Upholds/BindsTo restarts included.",
                  round(now_wall - (now_mono - mono / 1e6), 3), lab)


# ------------------------------------------------------------------ main

REDIS_COLLECTORS = [
    ("device", collect_device), ("ports", collect_ports), ("counters", collect_counters),
    ("portchannels", collect_portchannels), ("bfd", collect_bfd), ("bgp", collect_bgp),
    ("lldp", collect_lldp), ("transceivers", collect_transceivers), ("acl", collect_acl),
    ("copp", collect_copp), ("routes", collect_routes),
]


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--db-config", default=DB_CONFIG, help="SONiC database_config.json (DB ids, separators, socket)")
    ap.add_argument("--socket", help="redis unix socket (default: from --db-config, else %s)" % SOCKET)
    ap.add_argument("--host", help="redis TCP host when the socket fails (default: from --db-config)")
    ap.add_argument("--port", type=int)
    ap.add_argument("--timeout", type=float, default=3.0, help="redis socket timeout, seconds")
    ap.add_argument("--conflicts", default=CONFLICTS)
    ap.add_argument("--systemctl", default="systemctl", help="systemctl binary ('' skips the services)")
    args = ap.parse_args(argv)

    t0 = time.monotonic()
    o = Out()
    dbs, (sock, host, port) = load_dbs(args.db_config)
    r, err = connect(args.socket or sock, args.host or host, args.port or port, args.timeout)
    o.add("sonic_up", "1 when the SONiC redis answered.", int(r is not None))
    results = {}
    if r is None:
        print("sonic-metrics: redis: %s" % err, file=sys.stderr)
    else:
        s = Sonic(r, dbs)
        ctx = {}
        try:
            ctx["ports"] = [(n, c.get("alias", n)) for n, c in sorted(s.hashes("CONFIG_DB", "PORT").items(), key=port_order)]
        except (OSError, RedisError, ConnectionError) as e:
            print("sonic-metrics: CONFIG_DB PORT: %s" % e, file=sys.stderr)
            ctx["ports"] = []
        ctx["alias"] = dict(ctx["ports"])
        for name, fn in REDIS_COLLECTORS:
            try:
                fn(s, o, ctx)
                results[name] = 1
            except (OSError, RedisError, ConnectionError, ValueError, KeyError, TypeError) as e:
                print("sonic-metrics: %s: %s" % (name, e), file=sys.stderr)
                results[name] = 0
        r.close()
    try:
        collect_conflicts(o, args.conflicts)
        results["conflicts"] = 1
    except (OSError, ValueError, AttributeError) as e:
        print("sonic-metrics: conflicts: %s" % e, file=sys.stderr)
        results["conflicts"] = 0
    if args.systemctl:
        try:
            collect_services(o, args.systemctl)
            results["services"] = 1
        except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as e:
            print("sonic-metrics: services: %s" % e, file=sys.stderr)
            results["services"] = 0
    for name, v in results.items():
        o.add("sonic_collector_success", "1 when the collector read its source without error.", v, {"collector": name})
    o.add("sonic_collect_duration_seconds", "Run time of this collection (Python start-up excluded).",
          round(time.monotonic() - t0, 4))
    sys.stdout.write(o.render())
    return 0


if __name__ == "__main__":
    sys.exit(main())
