#!/usr/bin/env python3
"""cs8210-macsec: MACsec on the CS8210's Credo PHYs (ports 1-16).

Talks to the cs8210-macsec shim inside the vendor phy_server through the
vendor's own command FIFO (the protocol phy_shell uses), one command at a
time. Keys never go through the FIFO or argv: each SA's key material goes
into a 0600 file the shim reads once and deletes. Nothing here prints keys.

  cs8210-macsec status [--slice N] [--json]      MACsec info per port
  cs8210-macsec metrics                          Prometheus text on stdout
  cs8210-macsec keygen --loop A:B --cipher C --out FILE
  cs8210-macsec up --config FILE [--slice N ...] static SAs from FILE
  cs8210-macsec down --slice N                   back to the vendor bypass (clear!)
  cs8210-macsec rekey --config FILE --link A:B   hitless AN rollover, both ends local
  cs8210-macsec pn --slice N --dir tx|rx --an A [--sci S] [--set PN]
  cs8210-macsec hello

Static config (JSON, root-only, outside the Nix store), per port:
  {"ports": {"1": {"sci": "0e48c67a55cf0001", "cipher": "gcm-aes-xpn-256",
                   "window": 0, "conf_offset": 0,
                   "tx": {"an": 0, "sak": HEX, "salt": HEX, "ssci": "00000001"},
                   "rx": [{"sci": "...", "an": 0, "sak": HEX, "salt": HEX, "ssci": "00000002"}]}}}
salt/ssci only for the XPN ciphers. Details: ai_re/devices/cs8210-32x/macsec.md.
"""
import argparse
import errno
import fcntl
import json
import os
import re
import secrets
import stat
import sys
import time

SDKTMP = os.environ.get('CS8210_SDK_TMP', '/run/cs8210-sdk/tmp')
SDKRUN = os.environ.get('CS8210_SDK_RUN', '/run/cs8210-sdk/run')
FIFO = SDKTMP + '/cls_phy_fifo'
RUNNING = SDKTMP + '/cls_phy_cmd_running'
DONE = SDKTMP + '/cls_phy_cmd_done'
BASE = SDKTMP + '/cs8210-macsec'
KEYS = BASE + '/keys'
# mapped as the chroot bind mount (cs8210-sdk-chroot), or by its own name
SHIM_NAMES = ('libcs8210-phy-preload.so', 'cs8210-macsec-shim')

CIPHERS = {'gcm-aes-128': (16, False), 'gcm-aes-256': (32, False),
           'gcm-aes-xpn-128': (16, True), 'gcm-aes-xpn-256': (32, True)}
# Everything we put on the FIFO: phy_server runs unknown commands with
# system(), so nothing outside this alphabet may ever reach it.
SAFE = re.compile(r'^cmx_[a-z_]+( [a-z_]+=[A-Za-z0-9_.:-]+)*$')


class Error(Exception):
    pass


def hash_subkey(sak):
    """GCM hash subkey H = AES_K(0^128), what the engine calls 'auth'."""
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    enc = Cipher(algorithms.AES(sak), modes.ECB()).encryptor()
    return enc.update(bytes(16)) + enc.finalize()


def phy_server_pid():
    try:
        pid = int(open(SDKRUN + '/phy_server.pid').read().strip())
        os.kill(pid, 0)
        return pid
    except (OSError, ValueError):
        return None


def ensure_dirs():
    for d in (BASE, KEYS):
        try:
            os.mkdir(d, 0o700)
        except FileExistsError:
            pass
        st = os.lstat(d)
        if not stat.S_ISDIR(st.st_mode) or st.st_uid != 0:
            raise Error('%s is not a root-owned directory' % d)
        if st.st_mode & 0o077:
            os.chmod(d, 0o700)


class Phy:
    """One phy_server command at a time, the way phy_shell does it."""

    def __init__(self, timeout=60.0):
        if os.geteuid() != 0:
            raise Error('must run as root')
        self.timeout = timeout
        pid = phy_server_pid()
        if pid is None:
            raise Error('phy_server is not running (systemctl start cs8210-switch)')
        try:
            maps = open('/proc/%d/maps' % pid).read()
        except OSError as e:
            raise Error('cannot read phy_server maps: %s' % e)
        if not any(n in maps for n in SHIM_NAMES):
            raise Error('phy_server runs without the MACsec shim (hardware.cs8210.macsec.enable, '
                        'then restart cs8210-switch)')
        ensure_dirs()
        st = os.lstat(FIFO)
        if not stat.S_ISFIFO(st.st_mode) or st.st_uid != 0:
            raise Error('%s is not root-owned fifo' % FIFO)
        if st.st_mode & 0o077:
            os.chmod(FIFO, 0o600)  # command lines are none of anyone's business

    def cmd(self, name, **kw):
        # one flock per command (shared with the wpa_supplicant driver), so
        # a long-lived Phy (rekey with its drain sleep) does not block others
        with open(BASE + '/lock', 'a') as lockf:
            fcntl.flock(lockf, fcntl.LOCK_EX)
            return self._cmd(name, **kw)

    def _cmd(self, name, **kw):
        req = int.from_bytes(os.urandom(6), 'big')
        parts = [name] + ['%s=%s' % (k, v) for k, v in kw.items() if v is not None] + ['req=%d' % req]
        line = ' '.join(parts)
        if not SAFE.match(line) or len(line) > 1000:
            raise Error('refusing to send %r' % line)
        resp = '%s/r-%d' % (BASE, req)
        deadline = time.monotonic() + self.timeout
        # cls_phy_cmd_running is phy_shell's lock (a fifo used as a flag)
        while True:
            try:
                os.mkfifo(RUNNING, 0o600)
                break
            except FileExistsError:
                if time.monotonic() > deadline:
                    raise Error('%s held for %ds (stale? remove it if no phy_shell runs)'
                                % (RUNNING, self.timeout))
                time.sleep(0.02)
        try:
            try:
                os.unlink(DONE)
            except FileNotFoundError:
                pass
            try:
                fd = os.open(FIFO, os.O_WRONLY | os.O_NONBLOCK)
            except OSError as e:
                raise Error('cannot open %s: %s' % (FIFO, e))
            try:
                os.set_blocking(fd, True)
                data = (line + '\r').encode()
                while data:
                    data = data[os.write(fd, data):]
            finally:
                os.close(fd)
            while not os.path.exists(DONE):
                if time.monotonic() > deadline:
                    raise Error('no answer to %s within %ds' % (name, self.timeout))
                time.sleep(0.005)
            try:
                os.unlink(DONE)
            except FileNotFoundError:
                pass
        finally:
            try:
                os.unlink(RUNNING)
            except FileNotFoundError:
                pass
        try:
            with open(resp) as f:
                lines = f.read().splitlines()
            os.unlink(resp)
        except FileNotFoundError:
            raise Error('%s: no response file (shim not loaded or command rejected)' % name)
        if not lines or not lines[-1].startswith('end rc='):
            raise Error('%s: truncated response' % name)
        body = lines[:-1]
        rc = int(lines[-1].split('=', 1)[1])
        if rc or (body and body[0].startswith('err')):
            raise Error('%s: %s' % (name, ' '.join(body) or 'rc=%d' % rc))
        return [parse_line(l) for l in body]

    def keyfile(self, sak, hkey, salt=None):
        token = secrets.token_hex(16)
        path = KEYS + '/' + token
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'w') as f:
            f.write('sak=%s\nhkey=%s\n' % (sak.hex(), hkey.hex()))
            if salt is not None:
                f.write('salt=%s\n' % salt.hex())
        return token, path

    def sa_add(self, slice_, direction, an, sci, cipher, sak, pn=1, salt=None, ssci=None, enable=1):
        klen, xpn = CIPHERS[cipher]
        if len(sak) != klen:
            raise Error('SAK length %d does not fit %s' % (len(sak), cipher))
        if xpn and (salt is None or ssci is None):
            raise Error('%s needs salt and ssci' % cipher)
        token, path = self.keyfile(sak, hash_subkey(sak), salt if xpn else None)
        try:
            return self.cmd('cmx_sa_add', slice=slice_, dir=direction, an=an, sci=sci, cipher=cipher,
                            key=token, pn=pn & 0xffffffff, pnhi=(pn >> 32) if xpn else None,
                            ssci=ssci if xpn else None, enable=enable)
        finally:
            try:
                os.unlink(path)  # the shim deletes it on read; this covers errors
            except FileNotFoundError:
                pass


def parse_line(l):
    kind, _, rest = l.partition(' ')
    d = {'_': kind}
    for tok in rest.split():
        k, eq, v = tok.partition('=')
        if eq:
            d[k] = int(v) if re.fullmatch(r'\d+', v) else v
    return d


# ---- config ----

def load_config(path):
    st = os.stat(path)
    if st.st_mode & 0o077:
        raise Error('%s is readable by others; chmod 600 it' % path)
    with open(path) as f:
        return json.load(f)


def save_config(path, cfg):
    tmp = path + '.tmp'
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, 'w') as f:
        json.dump(cfg, f, indent=1)
        f.flush()
        os.fsync(f.fileno())
    os.rename(tmp, path)


def port_mac(slice_):
    path = '%s/fpp%d/address' % (os.environ.get('CS8210_NET_SYSFS', '/sys/class/net'), slice_)
    try:
        with open(path) as f:
            return f.read().strip().replace(':', '')
    except OSError as e:
        raise Error('no MAC for fpp%d (%s); is the SDK up?' % (slice_, e))


def new_key(cipher):
    klen, xpn = CIPHERS[cipher]
    k = {'sak': secrets.token_bytes(klen).hex()}
    if xpn:
        k['salt'] = secrets.token_bytes(12).hex()
    return k


def b(h):
    return bytes.fromhex(h) if h is not None else None


def cmd_keygen(a):
    x, y = (int(v) for v in a.loop.split(':'))
    if os.path.exists(a.out) and not a.force:
        raise Error('%s exists (--force to overwrite)' % a.out)
    sci = {x: port_mac(x) + '0001', y: port_mac(y) + '0001'}
    ssci = {x: '00000001', y: '00000002'}
    cfg = {'ports': {}}
    k = {x: new_key(a.cipher), y: new_key(a.cipher)}  # k[p] = what p transmits with
    for me, peer in ((x, y), (y, x)):
        tx = dict(k[me], an=0)
        rx = dict(k[peer], an=0, sci=sci[peer])
        if CIPHERS[a.cipher][1]:
            tx['ssci'], rx['ssci'] = ssci[me], ssci[peer]
        cfg['ports'][str(me)] = {'sci': sci[me], 'cipher': a.cipher, 'window': a.window,
                                 'conf_offset': 0, 'tx': tx, 'rx': [rx]}
    save_config(a.out, cfg)
    print('wrote %s (ports %d and %d, %s)' % (a.out, x, y, a.cipher))


def port_up(phy, s, pc, force=False):
    phy.cmd('cmx_port_up', slice=s, force=1 if force else None)
    phy.cmd('cmx_policy', slice=s, protect=1, encrypt=1, include_sci=1, replay=1,
            window=pc.get('window', 0), validate=2, conf_offset=pc.get('conf_offset', 0),
            controlled=1, open=0)


def cmd_up(a):
    cfg = load_config(a.config)
    phy = Phy()
    slices = a.slice or sorted(int(p) for p in cfg['ports'])
    for s in slices:
        port_up(phy, s, cfg['ports'][str(s)], a.force)
    # all receivers before any transmitter: no frame arrives without its SA
    for s in slices:
        pc = cfg['ports'][str(s)]
        for r in pc.get('rx', []):
            phy.sa_add(s, 'rx', r['an'], r['sci'], pc['cipher'], b(r['sak']), pn=r.get('pn', 1),
                       salt=b(r.get('salt')), ssci=r.get('ssci'))
    for s in slices:
        pc = cfg['ports'][str(s)]
        t = pc['tx']
        phy.sa_add(s, 'tx', t['an'], pc['sci'], pc['cipher'], b(t['sak']), pn=t.get('pn', 1),
                   salt=b(t.get('salt')), ssci=t.get('ssci'))
    print('MACsec up on slice(s) %s' % ' '.join(map(str, slices)))


def cmd_down(a):
    phy = Phy()
    for s in a.slice:
        phy.cmd('cmx_port_down', slice=s)
        print('slice %d: back to vendor bypass (no encryption)' % s)


def cmd_rekey(a):
    """Hitless rekey of a link whose two ends are on this box: per direction,
    new SAK on AN+1, receiver first, then the transmitter switches
    (SecY_SA_Chain + SecY_SA_Switch), then the old SAs go."""
    cfg = load_config(a.config)
    x, y = (int(v) for v in a.link.split(':'))
    phy = Phy()
    for src, dst in ((x, y), (y, x)):
        ps, pd = cfg['ports'][str(src)], cfg['ports'][str(dst)]
        c = ps['cipher']
        old = ps['tx']['an']
        an = (old + 1) % 4
        k = new_key(c)
        rx = [r for r in pd['rx'] if r['sci'] == ps['sci']]
        if not rx:
            raise Error('port %d has no rx entry for %d' % (dst, src))
        ssci_rx = rx[0].get('ssci')
        phy.sa_add(dst, 'rx', an, ps['sci'], c, b(k['sak']), pn=1, salt=b(k.get('salt')), ssci=ssci_rx)
        phy.sa_add(src, 'tx', an, ps['sci'], c, b(k['sak']), pn=1, salt=b(k.get('salt')),
                   ssci=ps['tx'].get('ssci'), enable=1)
        time.sleep(a.drain)  # frames already on the wire with the old AN
        phy.cmd('cmx_sa_del', slice=dst, dir='rx', an=old, sci=ps['sci'])
        phy.cmd('cmx_sa_del', slice=src, dir='tx', an=old)
        ps['tx'] = dict(k, an=an, ssci=ps['tx'].get('ssci')) if CIPHERS[c][1] else dict(k, an=an)
        pd['rx'] = [r for r in pd['rx'] if r['sci'] != ps['sci']] + [
            dict(k, an=an, sci=ps['sci'], ssci=ssci_rx) if CIPHERS[c][1] else dict(k, an=an, sci=ps['sci'])]
        save_config(a.config, cfg)
        print('%d -> %d: AN %d -> %d' % (src, dst, old, an))


def cmd_pn(a):
    phy = Phy()
    r = phy.cmd('cmx_sa_pn', slice=a.slice, dir=a.dir, an=a.an, sci=a.sci,
                set=None if a.set is None else a.set & 0xffffffff,
                sethi=None if a.set is None else a.set >> 32)
    print(' '.join('%s=%s' % kv for kv in r[0].items() if kv[0] != '_'))


def cmd_hello(a):
    for r in Phy().cmd('cmx_hello'):
        print(' '.join('%s=%s' % kv for kv in r.items() if kv[0] != '_'))


def collect(slice_=None):
    rows = Phy().cmd('cmx_status', slice=slice_)
    ports = {}
    for r in rows[1:]:
        s = r.get('slice')
        if s is None:
            continue
        p = ports.setdefault(s, {'slice': s, 'rules': [], 'secy': {}, 'ifc': {}, 'sa': []})
        kind = r.pop('_')
        r.pop('slice', None)
        if kind == 'port':
            p.update(r)
        elif kind == 'rule':
            p['rules'].append(r)
        elif kind in ('secy', 'ifc'):
            p[kind][r.pop('dir')] = r
        elif kind == 'sa':
            p['sa'].append(r)
    return ports


def cmd_status(a):
    ports = collect(a.slice)
    if a.json:
        json.dump(ports, sys.stdout, indent=1)
        print()
        return
    for s in sorted(ports):
        p = ports[s]
        if p.get('mode') != 'macsec':
            if a.slice or p.get('mode') != 'bypass':
                print('port %d: %s' % (s, p.get('mode')))
            continue
        print('port %d: MACsec, channel %s, controlled port %s%s' % (
            s, p['channel'], 'on' if p['controlled'] else 'off', ', OPEN (unprotected allowed)' if p['open'] else ''))
        print('  tx: protect=%s encrypt=%s conf_offset=%s include_sci=%s es=%s scb=%s data %s' % (
            p['protect'], p['encrypt'], p['conf_offset'], p['include_sci'], p['es'], p['scb'],
            'DROPPED' if p['tx_drop'] else 'passing'))
        print('  rx: validate=%s replay=%s window=%s data %s' % (
            p['validate'], p['replay'], p['window'], 'DROPPED' if p['rx_drop'] else 'passing'))
        for r in p['rules']:
            print('  rule %s/%s index %s hits %s%s' % (r['dir'], r['kind'], r['index'], r['hits'],
                                                      ' (drop)' if r['drop'] else ''))
        for d in ('tx', 'rx'):
            for k, v in sorted(p['secy'].get(d, {}).items()):
                if v:
                    print('  %s secy %s=%s' % (d, k, v))
        for sa in p['sa']:
            xpn = 'xpn' in sa['cipher']
            used = sa.get('next_pn', 0) / float(1 << (64 if xpn else 32))
            ctr = {k: v for k, v in sa.items() if k.startswith(('In', 'Out'))}
            print('  sa %s an=%s sci=%s %s %s next_pn=%s (%.3f%% of PN space) age=%ss' % (
                sa['dir'], sa['an'], sa['sci'], sa['cipher'], sa['state'], sa.get('next_pn', '-'),
                100 * used, sa.get('age', '-')))
            print('     ' + ' '.join('%s=%s' % kv for kv in sorted(ctr.items()) if kv[1]))


def cmd_metrics(a):
    out = []

    def m(name, help_, typ, samples):
        out.append('# HELP %s %s' % (name, help_))
        out.append('# TYPE %s %s' % (name, typ))
        for labels, v in samples:
            out.append('%s{%s} %s' % (name, ','.join('%s="%s"' % kv for kv in labels.items()), v))

    try:
        ports = collect()
        ok = 1
    except Error as e:
        print('cs8210-macsec metrics: %s' % e, file=sys.stderr)
        ports, ok = {}, 0
    m('cs8210_macsec_scrape_ok', 'MACsec shim answered (0 = SDK/shim not running).', 'gauge', [({}, ok)])
    live = [p for p in ports.values() if p.get('mode') == 'macsec']
    m('cs8210_macsec_port_up', 'Port has the MACsec engine on (1) or is in vendor bypass (0).', 'gauge',
      [({'port': str(s)}, int(p.get('mode') == 'macsec')) for s, p in sorted(ports.items())])
    m('cs8210_macsec_data_dropped', 'Data rule drops (controlled port closed or no SA).', 'gauge',
      [({'port': str(p['slice']), 'dir': d}, p[d + '_drop']) for p in live for d in ('tx', 'rx')])
    m('cs8210_macsec_rule_hits_total', 'Classifier TCAM hits per rule.', 'counter',
      [({'port': str(p['slice']), 'dir': r['dir'], 'kind': r['kind']}, r['hits']) for p in live for r in p['rules']])
    m('cs8210_macsec_secy_packets_total', 'Per-SecY (vPort) counters.', 'counter',
      [({'port': str(p['slice']), 'dir': d, 'counter': k}, v) for p in live
       for d, c in sorted(p['secy'].items()) for k, v in sorted(c.items())])
    m('cs8210_macsec_ifc_total', 'Per-interface counters (controlled/uncontrolled).', 'counter',
      [({'port': str(p['slice']), 'dir': d, 'counter': k}, v) for p in live
       for d, c in sorted(p['ifc'].items()) for k, v in sorted(c.items())])
    sas = [(p, sa) for p in live for sa in p['sa'] if 'next_pn' in sa]
    lab = lambda p, sa: {'port': str(p['slice']), 'dir': sa['dir'], 'an': str(sa['an']), 'sci': sa['sci'],
                         'cipher': sa['cipher'], 'state': sa['state']}
    m('cs8210_macsec_sa_next_pn', 'Next packet number (tx: next to send, rx: next expected).', 'gauge',
      [(lab(p, sa), sa['next_pn']) for p, sa in sas])
    m('cs8210_macsec_sa_pn_used_ratio', 'Fraction of the PN space used (rekey well before 1).', 'gauge',
      [(lab(p, sa), '%.9f' % (sa['next_pn'] / float(1 << (64 if 'xpn' in sa['cipher'] else 32)))) for p, sa in sas])
    m('cs8210_macsec_sa_age_seconds', 'Seconds since the SA was installed.', 'gauge',
      [(lab(p, sa), sa['age']) for p, sa in sas])
    m('cs8210_macsec_sa_packets_total', 'Per-SA counters (In*/Out*).', 'counter',
      [(dict(lab(p, sa), counter=k), v) for p, sa in sas for k, v in sorted(sa.items())
       if k.startswith(('In', 'Out'))])
    print('\n'.join(out))


def main():
    ap = argparse.ArgumentParser(prog='cs8210-macsec', description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('status')
    p.add_argument('--slice', type=int)
    p.add_argument('--json', action='store_true')
    p.set_defaults(fn=cmd_status)
    sub.add_parser('metrics').set_defaults(fn=cmd_metrics)
    sub.add_parser('hello').set_defaults(fn=cmd_hello)
    p = sub.add_parser('keygen')
    p.add_argument('--loop', required=True, help='A:B, two ports of this box cabled to each other')
    p.add_argument('--cipher', choices=sorted(CIPHERS), default='gcm-aes-xpn-256')
    p.add_argument('--window', type=int, default=0)
    p.add_argument('--out', required=True)
    p.add_argument('--force', action='store_true')
    p.set_defaults(fn=cmd_keygen)
    p = sub.add_parser('up')
    p.add_argument('--config', required=True)
    p.add_argument('--slice', type=int, action='append')
    p.add_argument('--force', action='store_true', help='take over a slice someone else initialised')
    p.set_defaults(fn=cmd_up)
    p = sub.add_parser('down')
    p.add_argument('--slice', type=int, action='append', required=True)
    p.set_defaults(fn=cmd_down)
    p = sub.add_parser('rekey')
    p.add_argument('--config', required=True)
    p.add_argument('--link', required=True, help='A:B (both ends on this box)')
    p.add_argument('--drain', type=float, default=0.5, help='seconds before old SAs go')
    p.set_defaults(fn=cmd_rekey)
    p = sub.add_parser('pn')
    p.add_argument('--slice', type=int, required=True)
    p.add_argument('--dir', choices=['tx', 'rx'], required=True)
    p.add_argument('--an', type=int, required=True)
    p.add_argument('--sci')
    p.add_argument('--set', type=lambda v: int(v, 0))
    p.set_defaults(fn=cmd_pn)
    a = ap.parse_args()
    try:
        a.fn(a)
    except Error as e:
        print('cs8210-macsec: %s' % e, file=sys.stderr)
        sys.exit(1)


if __name__ == '__main__':
    main()
