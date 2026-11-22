"""MDIO backends for the Evora re-host: mock (register model, no hardware) and ssh (our bcm.user on the switch).

Trace line format (one per event, in order):
  R <phy> <devad> <reg> <value>     clause-45 read and the value returned
  W <phy> <devad> <reg> <value>     clause-45 write (16 bit)
  D <usec>                          delay requested by the vendor code
  # ...                             comment / marker
All numbers hex except the delay.
"""
import re, subprocess, sys, time, select, os


class Trace:
    def __init__(self, path=None):
        self.f = open(path, 'w') if path else None
        self.n = {'R': 0, 'W': 0, 'D': 0}

    def ev(self, kind, *a):
        self.n[kind] = self.n.get(kind, 0) + 1
        if self.f:
            if kind == 'D':
                self.f.write('D %d\n' % a[0])
            else:
                self.f.write('%s %03x %x %04x %04x\n' % ((kind,) + a))

    def note(self, s):
        if self.f:
            for l in s.splitlines():
                self.f.write('# ' + l + '\n')
            self.f.flush()

    def close(self):
        if self.f:
            self.f.close()


class Mock:
    """Register store with the ID registers preset. Not a chip model: polls time out."""
    real_sleep = False

    def __init__(self, ids=None):
        self.regs = {}
        self.ids = ids

    def read(self, phy, devad, reg):
        if self.ids is not None and phy not in self.ids:
            return 0xffff
        if devad == 1 and reg == 0x8b00:
            return 0x2391
        if devad == 1 and reg == 0x8b01:
            return 0x80a0
        return self.regs.get((phy, devad, reg), 0)

    def write(self, phy, devad, reg, val):
        self.regs[(phy, devad, reg)] = val & 0xffff

    def close(self):
        pass


class Ssh:
    """Long-lived 'bcm.user' diag shell on the switch, driven over one ssh connection.

    read : phy raw c45 <id> <devad> <reg>          -> prints "\t0x<reg>: 0x<value>"
    write: phy raw c45 <id> <devad> <reg> <value>  -> prints nothing
    The shell prints no prompt on a pipe, so every synchronisation point is an 'echo MK<n>' marker.
    Writes are pipelined (no round trip); a read or flush() waits for the marker.
    """
    real_sleep = True

    def __init__(self, cmd, cwd_cmd='cd /tmp/phy && exec stdbuf -o0 -e0 bcm.user', init=('init soc', 'init misc'), log=None,
                 expect='attached'):
        self.log = open(log, 'ab') if log else None
        self.p = subprocess.Popen(cmd + [cwd_cmd], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.STDOUT, bufsize=0)
        self.buf = b''
        self.seq = 0
        self.dirty = False
        self.banner = self.cmd('\n'.join(init), 120)
        if expect not in self.banner:
            raise RuntimeError('bridge: diag shell did not attach: %r' % self.banner[-400:])

    def _fill(self, timeout):
        r, _, _ = select.select([self.p.stdout], [], [], timeout)
        if not r:
            raise TimeoutError('bridge: no output for %ss; buffer tail %r' % (timeout, self.buf[-200:]))
        d = os.read(self.p.stdout.fileno(), 65536)
        if not d:
            raise EOFError('bridge closed; tail %r' % self.buf[-400:])
        if self.log:
            self.log.write(d); self.log.flush()
        self.buf += d

    def cmd(self, line, timeout=20):
        """Send line(s), return everything printed up to the marker (includes output of pipelined writes)."""
        self.seq += 1
        mark = b'MK%d' % self.seq
        self.p.stdin.write((line + '\n').encode() + b'echo ' + mark + b'\n')
        mark += b'\n'
        while True:
            i = self.buf.find(mark)
            if i >= 0:
                out = self.buf[:i].decode('latin1')
                self.buf = self.buf[i + len(mark):]
                self.dirty = False
                return out
            self._fill(timeout)

    def read(self, phy, devad, reg):
        out = self.cmd('phy raw c45 0x%x 0x%x 0x%x' % (phy, devad, reg))
        m = re.findall(r'^\s*0x([0-9a-fA-F]+): 0x([0-9a-fA-F]{4})\s*$', out, re.M)
        if len(m) != 1 or out.count('\n') != 1 or int(m[0][0], 16) != reg:
            raise RuntimeError('bridge: unexpected answer to a read (or noise from earlier writes): %r' % out)
        return int(m[0][1], 16)

    def write(self, phy, devad, reg, val):
        self.p.stdin.write(('phy raw c45 0x%x 0x%x 0x%x 0x%x\n' % (phy, devad, reg, val & 0xffff)).encode())
        self.dirty = True

    def flush(self):
        if self.dirty:
            out = self.cmd('')
            if out.strip():
                raise RuntimeError('bridge: diag shell printed after writes: %r' % out)

    def close(self):
        try:
            self.flush()
            self.p.stdin.write(b'exit\n')
            self.p.stdin.close()
            self.p.wait(10)
        except Exception:
            self.p.kill()


class Diag(Ssh):
    """The same line protocol through syncd's diag socket, with SONiC running: no second SDK process, no SDK lock.

    A shell loop on the switch hands every line to 'openbcm-diag' (one diag-shell command per connection, run by
    syncd under the SAI lock) and answers the 'echo MK<n>' markers itself. syncd has run 'init all', so the MDIO
    rate and hold delay are the config.bcm values (the "init misc first" rule holds); that is checked: the first
    command reads CMIC_MIIM_CONFIG-independent proof, the chip ID of PHY 0x00, and refuses anything but 0x2391.
    About 6 ms per access. Only one user at a time: the vendor library's indirect (LMI) accesses are multi-step.
    """
    LOOP = ('while IFS= read -r l; do case "$l" in "echo MK"*) echo "${l#echo }";; "") ;; '
            '*) openbcm-diag "$l" 2>&1;; esac; done')

    def __init__(self, cmd, log=None, sock=None):
        # sock: another server speaking the same protocol (loadtest/bcmd.py: a private bcm.user, SONiC stopped)
        sock = sock or os.environ.get('EVORA_DIAG_SOCKET')
        loop = ('export OPENBCM_DIAG_SOCKET=%s; ' % sock if sock else '') + self.LOOP
        Ssh.__init__(self, cmd, cwd_cmd=loop, init=('phy raw c45 0x0 0x1 0x8b00',), log=log, expect='0x8b00: 0x2391')

    def close(self):
        # no 'exit': the server is not ours to end (syncd refuses it; it ended the first bcmd.py, 2026-11-22)
        try:
            self.flush()
            self.p.stdin.close()
            self.p.wait(10)
        except Exception:
            self.p.kill()


def make_backend(name, args):
    if name == 'mock':
        return Mock()
    if name == 'ssh':
        return Ssh(args.ssh.split(), log=args.bridge_log)
    if name == 'diag':
        return Diag(args.ssh.split(), log=args.bridge_log)
    raise SystemExit('unknown backend ' + name)
