"""Evora LMI (indirect register) protocol: decoder for MDIO traces and the register model of the mock backend.

Vendor code: plp_evora_lmi_reg_access 0x11826108, command builder 0x11825d1c, address classes 0x1182593c
(see ../macsec.md). All on clause 45 device 1:
  1.8b90  side select (0 = line, 1 = system), written before an operation
  1.900a  status: bit 0 done (clears on read), bits 2:1 read data ready, bits 8:3 errors
  1.9005  command / data FIFO, 16-bit words:
          cmd0  [15:12] lane mask  [11:8] block  [7:6] address bits 17:16 (MACsec blocks)  [3] write  [2] burst
                [0] cmd1 follows
          cmd1  (only if cmd0 bit 0)  [12:8] register width in bytes  [7:0] register count
          data  (write only) count * width/2 words, least significant word first
          addr  address bits 15:0
          then, for a read, count * width/2 reads of 1.9005 return the data, least significant word first.
Block nibble <- address class (bits 31:24 of the library's register address):
  1 / 9 / f : 0x40 (32 bit), 0x41 (64 bit: port MAC, MIB), 0x42 (128 bit)   (9 = line side, f = both, 1 = system)
  2 : 0x43    3 : 0x44    a : 0x45    b : 0x46   (32 bit; MACsec classifier / transform engines)     6 : 0x49 (16 bit)
"""
LMI_STATUS, LMI_FIFO, LMI_SIDE = 0x900a, 0x9005, 0x8b90
BLOCK_NAME = {1: 'pm', 9: 'pm', 0xf: 'pm', 2: '43', 3: '44', 0xa: '45', 0xb: '46', 6: '49', 0: '??'}


def width_of(cmd0, cmd1):
    if cmd0 & 1:
        return (cmd1 >> 8) & 0x1f, cmd1 & 0xff
    blk = (cmd0 >> 8) & 0xf
    return (2 if blk == 6 else 4), 1


class Op:
    """One decoded LMI operation."""
    __slots__ = ('phy', 'side', 'cmd0', 'cmd1', 'addr', 'write', 'width', 'count', 'words', 'line')

    @property
    def block(self):
        return (self.cmd0 >> 8) & 0xf

    @property
    def lane(self):
        return (self.cmd0 >> 12) & 0xf

    @property
    def full_addr(self):
        """Address with bits 17:16 for the MACsec blocks."""
        return self.addr | (((self.cmd0 >> 6) & 3) << 16 if self.block in (3, 0xb) else 0)

    def values(self):
        n = self.width // 2
        out = []
        for i in range(self.count):
            v = 0
            for k, w in enumerate(self.words[i * n:(i + 1) * n]):
                v |= w << (16 * k)
            out.append(v)
        return out

    def key(self):
        return (self.phy, self.side if self.block in (1, 9, 0xf) else 0, self.block, self.lane, self.full_addr)

    def __str__(self):
        return '%s %03x side%d blk%x lane%x %05x %s' % (
            'W' if self.write else 'R', self.phy, self.side, self.block, self.lane, self.full_addr,
            ' '.join('%0*x' % (self.width * 2, v) for v in self.values()))


class Parser:
    """Feed the 1.9005 / 1.8b90 accesses of one PHY in order; complete operations are returned."""

    def __init__(self, phy):
        self.phy, self.side, self.w, self.op = phy, 0, [], None

    def need(self):
        """Number of command words a write/read command takes in total, or None if not known yet."""
        w = self.w
        if not w:
            return None
        n = 1
        if w[0] & 1:
            if len(w) < 2:
                return None
            n = 2
        width, count = width_of(w[0], w[1] if n == 2 else 0)
        data = count * (width // 2) if w[0] & 8 else 0
        return n + data + 1

    def write(self, reg, val, line=0):
        if reg == LMI_SIDE:
            self.side = val & 1
            return None
        if reg != LMI_FIFO:
            return None
        self.w.append(val)
        n = self.need()
        if n is None or len(self.w) < n:
            return None
        w, self.w = self.w, []
        op = Op()
        op.phy, op.side, op.cmd0, op.line = self.phy, self.side, w[0], line
        op.cmd1 = w[1] if w[0] & 1 else 0
        op.width, op.count = width_of(op.cmd0, op.cmd1)
        op.write = bool(w[0] & 8)
        op.addr = w[-1]
        op.words = w[(2 if w[0] & 1 else 1):-1]
        if op.write:
            return op
        self.op = op          # a read: data follows as FIFO reads
        return None

    def read(self, reg, val):
        op = self.op
        if reg != LMI_FIFO or op is None:
            return None
        op.words.append(val)
        if len(op.words) == op.count * (op.width // 2):
            self.op = None
            return op
        return None


def decode(lines):
    """Yield Op for every LMI operation in an MDIO trace (lines of rehost.py --trace)."""
    ps = {}
    for n, l in enumerate(lines, 1):
        f = l.split()
        if len(f) != 5 or f[0] not in 'RW' or f[2] != '1':
            continue
        phy, reg, val = int(f[1], 16), int(f[3], 16), int(f[4], 16)
        p = ps.get(phy) or ps.setdefault(phy, Parser(phy))
        op = p.write(reg, val, n) if f[0] == 'W' else p.read(reg, val)
        if op:
            yield op


class Model:
    """LMI register store for the mock backend: writes are kept, reads return what was written (or a preset, or 0)."""

    def __init__(self, preset=None):
        self.ps, self.regs, self.status, self.fifo = {}, dict(preset or {}), {}, {}
        self.log = []

    def handles(self, devad, reg):
        return devad == 1 and reg in (LMI_STATUS, LMI_FIFO, LMI_SIDE)

    def write(self, phy, reg, val):
        p = self.ps.get(phy) or self.ps.setdefault(phy, Parser(phy))
        if reg == LMI_SIDE:
            p.write(reg, val)
            self.regs[('side', phy)] = val
            return
        op = p.write(reg, val)
        if op:                                   # a complete write
            n = op.width // 2
            for i in range(op.count):
                k = op.key()[:4] + (op.full_addr + i,)
                self.regs[k] = op.words[i * n:(i + 1) * n]
            self.status[phy] = 0x0001
            self.log.append(op)
        elif p.op is not None and not self.fifo.get(phy):   # a complete read command: queue the data
            op = p.op
            n = op.width // 2
            q = []
            for i in range(op.count):
                k = op.key()[:4] + (op.full_addr + i,)
                q += self.value(k, n)
            self.fifo[phy] = q
            op.words = list(q)
            p.op = None
            self.status[phy] = 0x0007
            self.log.append(op)

    def value(self, k, n):
        v = self.regs.get(k)
        if v is None:
            v = self.regs.get(('any',) + k[1:])     # preset learned from another PHY
        v = list(v) if v is not None else []
        return (v + [0] * n)[:n]

    def read(self, phy, reg):
        if reg == LMI_SIDE:
            return self.regs.get(('side', phy), 0)
        if reg == LMI_STATUS:
            return self.status.pop(phy, 0)
        q = self.fifo.get(phy)
        return q.pop(0) if q else 0


if __name__ == '__main__':
    import sys, collections
    ops = list(decode(open(sys.argv[1])))
    if len(sys.argv) > 2 and sys.argv[2] == '-v':
        for op in ops:
            print(op.line, op)
    c = collections.Counter((op.block, op.write) for op in ops)
    for (b, w), n in sorted(c.items()):
        print('block %x %s: %d' % (b, 'writes' if w else 'reads', n))
