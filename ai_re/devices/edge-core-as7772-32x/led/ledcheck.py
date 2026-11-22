#!/usr/bin/env python3
"""Emulator for the Broadcom CMIC LED processor (the instruction set of the
OpenBCM SDK's tools/led/tools/ledasm.h / ledsim.c) and the offline checks of
the AS7772 LED programs. Run by build.sh; see ../leds.md.

  ledcheck.py <dir with led0.hex led1.hex pattern-*.hex> [factory dir]

Without the factory dir (devices/.../dumps/x/diag/root/usr/local/bin, private)
only the self-checks run.
"""
import itertools, random, sys
from pathlib import Path

ZERO, ONE = 0xE, 0xF


def load(path):
    b = [int(x, 16) for x in Path(path).read_text().split()]
    return b + [0] * (256 - len(b))


class Led:
    def __init__(self, prog):
        self.prog, self.data = prog, [0] * 256

    def status(self, port, bit):          # pushst: 2 status bytes per port at data[2*port]
        if bit == ZERO: return 0
        if bit == ONE: return 1
        return (self.data[2 * port + (bit >> 3 & 1)] >> (bit & 7)) & 1

    def tick(self):
        """Run one frame (pc 0 until send); returns the bits sent."""
        p, d = self.prog, self.data
        a = b = c = z = pc = port = 0
        stack, ret, bits = [], [], []
        for _ in range(100000):
            op, imm = p[pc], p[(pc + 1) & 255]
            ln = [1]

            def src(f):
                if f == 0: return a
                if f == 1: return b
                if f == 2: ln[0] = 2; return imm
                if f == 4: return d[a]
                if f == 5: return d[b]
                if f == 6: ln[0] = 2; return d[imm]
                if f == 7: return c
                raise ValueError("bad operand %02x at %02x" % (op, pc))

            def dst(f, v):
                nonlocal a, b
                v &= 255
                if f == 0: a = v
                elif f == 1: b = v
                elif f == 4: d[a] = v
                elif f == 5: d[b] = v
                elif f == 6: ln[0] = 2; d[imm] = v
                else: raise ValueError("bad destination %02x at %02x" % (op, pc))

            if op == 0x57:
                pc = ret.pop(); continue
            if op == 0x67:
                if len(ret) == 2: raise ValueError("call depth > 2 at %02x" % pc)
                ret.append(pc + 2); pc = imm; continue
            if op == 0x77: pc = imm; continue
            if op in (0x70, 0x71, 0x74, 0x75):
                take = {0x70: z, 0x71: c, 0x74: not z, 0x75: not c}[op]
                pc = imm if take else pc + 2; continue
            if op == 0x87: bits.append(stack.pop())
            elif op == 0x97: c = stack.pop()
            elif op == 0xB7: stack.append(stack.pop() | stack.pop())
            elif op == 0xC7: stack.append(stack.pop() & stack.pop())
            elif op == 0xA7: stack.append(stack.pop() ^ stack.pop())
            elif op == 0xD7: stack.append(stack.pop() ^ 1)
            elif op == 0x07: c = 0
            elif op == 0x17: c = 1
            elif op == 0x37: c ^= 1
            elif op & 0xF8 == 0x20: stack.append(src(op & 7) & 1)
            elif op & 0xF8 == 0x28: port = src(op & 7)
            elif op & 0xFC == 0x30: stack.append(self.status(port, src(op & 3)))
            elif op & 0xF8 == 0x38:
                n = src(op & 7)
                if len(bits) < n: raise ValueError("send %d with %d bits packed" % (n, len(bits)))
                return bits[:n]
            elif op & 0xF8 == 0x80: v = src(op & 7); c = z = int(v == 255); dst(op & 7, v + 1)
            elif op & 0xF8 == 0x90: v = src(op & 7); c = int(v == 0); z = int(v == 1); dst(op & 7, v - 1)
            elif op & 0xF8 == 0x98: v = src(op & 7); c = v & 1; z = int(v == 0); dst(op & 7, v >> 1 | v << 7)
            elif op & 0xF8 == 0x88: v = src(op & 7); c = v >> 7; z = int(v == 0); dst(op & 7, v << 1 | v >> 7)
            elif op & 0xF0 in (0xA0, 0xB0, 0xC0, 0xD0, 0xE0, 0xF0):
                s, t, k = src(op & 7), (b if op & 8 else a), op & 0xF0
                r = {0xA0: t ^ s, 0xB0: t | s, 0xC0: t & s, 0xD0: t - s, 0xE0: t - s, 0xF0: t + s}[k]
                if k in (0xD0, 0xE0): c = int(r < 0)
                if k == 0xF0: c = int(r > 255)
                z = int(r & 255 == 0)
                if k != 0xD0:
                    if op & 8: b = r & 255
                    else: a = r & 255
            elif op & 0x8C == 0x08: c = (src(op >> 4 & 7) >> (src(op & 3) & 7)) & 1
            elif op & 0x88 == 0x00: dst(op >> 4 & 7, src(op & 7))
            else: raise ValueError("opcode %02x at %02x" % (op, pc))
            pc += ln[0]
        raise ValueError("no send")


# Board facts (leds.md): LED index of lane 0 of each cage, in the order the
# cages are sent; index -> first SerDes lane.
ORDER = {0: [32, 36, 40, 44, 16, 20, 24, 28, 48, 52, 56, 60, 0, 4, 8, 12],
         1: [16, 20, 24, 28, 32, 36, 40, 44, 0, 4, 8, 12, 48, 52, 56, 60]}
FIRST_CAGE = {0: 17, 1: 1}
PATTERNS = {"blue": "LED_100G", "yellow": "LED_40G", "cyan": "LED_25G", "green": "LED_10G", "off": "LED_off"}


def cage_bits(link, lit=None):
    return [1, 0, 1, 0, int(link if lit is None else lit)] + [0] * 9


def run(prog, n, links, act=(), ticks=1, factory=False):
    m = Led(prog)
    out = []
    for _ in range(ticks):
        for k, idx in enumerate(ORDER[n]):
            if factory:      # what the factory bcm.user writes: bit 7 four lanes, bit 5 25G/100G, bit 0 link
                m.data[0xE0 + k] = idx
                m.data[0xA0 + idx] = 0xA0 | int(k in links)
            else:            # what the SDK's "led auto on" writes: bit 0
                m.data[0xA0 + idx] = int(k in links)
            m.data[2 * idx] = 3 if k in act else 0
        out.append(m.tick())
    return out


def main():
    built = Path(sys.argv[1])
    factory = Path(sys.argv[2]) if len(sys.argv) > 2 else None
    rnd = random.Random(7772)
    for n in (0, 1):
        prog = load(built / ("led%d.hex" % n))
        size = max(i for i, x in enumerate(prog) if x) + 1
        for trial in range(200):
            links = {k for k in range(16) if rnd.random() < 0.5}
            want = list(itertools.chain.from_iterable(cage_bits(k in links) for k in range(16)))
            got = run(prog, n, links)[0]
            assert got == want, "led%d: link pattern %s" % (n, sorted(links))
            if factory:
                fac = run(load(factory / "ES7632BTx.hex"), n, links, factory=True)[0]
                assert fac == got, "led%d differs from the factory program, links %s" % (n, sorted(links))
        # frames on cage slot 3 with link: 4 ticks on, 4 off; frames without link: dark
        frames = run(prog, n, {3}, act={3, 5}, ticks=16)
        lamp = [f[14 * 3 + 4] for f in frames]
        assert lamp == [1, 1, 1, 1, 0, 0, 0, 0] * 2, lamp
        assert all(f[14 * 5 + 4] == 0 for f in frames)
        print("led%d.hex: %d bytes, 224 bits/frame, cages %d-%d: ok%s" % (
            n, size, FIRST_CAGE[n], FIRST_CAGE[n] + 15,
            " (same bits as the factory program for 32x100G, 200 link patterns)" if factory else ""))
    for name, fac in PATTERNS.items():
        got = Led(load(built / ("pattern-%s.hex" % name))).tick()
        assert len(got) == 224
        if factory:
            assert got == Led(load(factory / (fac + ".hex"))).tick(), name
        print("pattern-%s.hex: %s  x16%s" % (name, "".join(map(str, got[:14])), " = factory " + fac if factory else ""))

    # Expected scan-out bytes (data RAM 0x80-0x9b) for the live check.
    def ram(n, links):
        bits = run(load(built / ("led%d.hex" % n)), n, links)[0]
        return " ".join("%02x" % sum(bits[i + j] << j for j in range(8)) for i in range(0, 224, 8))
    print("scan-out bytes 0x80..0x9b (bit n of the frame = bit n%8 of byte 0x80+n/8):")
    print("  LED1, cages 1+2 up :", ram(1, {0, 1}))
    print("  LED1, none up      :", ram(1, set()))
    print("  LED0, cages 31+32 up:", ram(0, {14, 15}))
    print("  LED0, none up      :", ram(0, set()))


if __name__ == "__main__":
    main()
