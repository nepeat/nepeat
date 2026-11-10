"""7-series frame-address geometry from a Project X-Ray part.json.

FAR layout (UG470 table 5-24):
    [25:23] block type   0 = CLB/IO/CLK (interconnect + logic config)
                         1 = BRAM content
                         2 = CFG_CLB (not present in prjxray part.json)
    [22]    half         0 = top, 1 = bottom
    [21:17] row          clock-region row inside the half
    [16:7]  column       configuration column (major)
    [6:0]   minor        frame inside the column

The FDRI auto-increment walks minor -> column -> row -> half (top then
bottom) -> block type.  At the end of every row the configuration stream
carries two extra frames that are not stored anywhere ("row padding");
they are modelled here as pseudo-addresses ('PAD', far_of_last_frame, i).
"""
import json

WORDS = 101

BUS = {'CLB_IO_CLK': 0, 'BLOCK_RAM': 1, 'CFG_CLB': 2}
BUSNAME = {v: k for k, v in BUS.items()}


def far(bt, bottom, row, col, minor):
    return bt << 23 | bottom << 22 | row << 17 | col << 7 | minor


def unfar(a):
    return ((a >> 23) & 7, (a >> 22) & 1, (a >> 17) & 0x1f, (a >> 7) & 0x3ff, a & 0x7f)


def farstr(a):
    bt, b, r, c, m = unfar(a)
    return '%s/%s/r%d/c%d/m%d' % (BUSNAME.get(bt, 'bt%d' % bt), 'bot' if b else 'top', r, c, m)


class Geometry:
    def __init__(self, part_json):
        p = json.load(open(part_json))
        self.idcode = p['idcode']
        # rows[(bt, bottom, row)] = [frame_count per column]
        self.rows = {}
        for half, h in p['global_clock_regions'].items():
            bottom = 1 if half == 'bottom' else 0
            for r, row in h['rows'].items():
                for bus, b in row['configuration_buses'].items():
                    cols = b['configuration_columns']
                    self.rows[(BUS[bus], bottom, int(r))] = [
                        cols[str(i)]['frame_count'] for i in range(len(cols))]
        # The linear configuration stream order, with 2 pad slots per row.
        self.seq = []
        for key in sorted(self.rows):
            bt, bottom, r = key
            last = None
            for c, n in enumerate(self.rows[key]):
                for m in range(n):
                    last = far(bt, bottom, r, c, m)
                    self.seq.append(last)
            self.seq.append(('PAD', last, 0))
            self.seq.append(('PAD', last, 1))
        self.index = {a: i for i, a in enumerate(self.seq)}
        self.valid_list = [a for a in self.seq if isinstance(a, int)]
        self.valid_index = {a: i for i, a in enumerate(self.valid_list)}
        self.valid = set(self.valid_list)

    def is_valid(self, a):
        return a in self.valid

    def frame_count(self):
        return len(self.valid)
