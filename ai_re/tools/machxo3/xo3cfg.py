#!/usr/bin/env python3
"""frames.txt (xo3bit.py) <-> Project Trellis text config, losslessly.

  xo3cfg.py unpack FRAMES.txt DEVICE OUT.config   e.g. DEVICE = LCMXO3LF-2100C
  xo3cfg.py pack   IN.config DEVICE FRAMES.txt
  xo3cfg.py check  FRAMES.txt DEVICE              unpack -> pack in memory

Needs pytrellis (PYTHONPATH=$TRELLIS/lib/trellis) and the Trellis database
($TRELLIS/share/trellis/database). Bits the database does not know stay in the
config as `unknown: F<frame>B<bit>` lines (Trellis' own convention), so the
config is lossless; `check` proves it on the given frames.

Frame byte order matches the bitstream (Trellis Bitstream.cpp): CRAM bit
(frame, j) = byte[nbytes-1 - j//8] bit (j % 8).
"""
import os, sys
import pytrellis

DB = os.path.join(os.environ.get('TRELLIS', ''), 'share/trellis/database')


def frames_to_chip(frames, device):
    chip = pytrellis.Chip(device)
    nb = chip.info.bits_per_frame
    nbytes = (nb + 7) // 8
    assert len(frames) == chip.info.num_frames, (len(frames), chip.info.num_frames)
    for f, fr in enumerate(frames):
        assert len(fr) == nbytes
        for j in range(nb):
            if (fr[nbytes - 1 - j // 8] >> (j % 8)) & 1:
                chip.cram.set_bit(f, j, True)
    return chip


def chip_to_frames(chip):
    nb = chip.info.bits_per_frame
    nbytes = (nb + 7) // 8
    out = []
    for f in range(chip.info.num_frames):
        fr = bytearray(nbytes)
        for j in range(nb):
            if chip.cram.bit(f, j):
                fr[nbytes - 1 - j // 8] |= 1 << (j % 8)
        out.append(bytes(fr))
    return out


def unpack(frames, device):
    return pytrellis.ChipConfig.from_chip(frames_to_chip(frames, device)).to_string()


def pack(text, device):
    cc = pytrellis.ChipConfig.from_string(text)
    chip = cc.to_chip()
    return chip_to_frames(chip)


def main():
    pytrellis.load_database(DB)
    op = sys.argv[1]
    rd = lambda p: [bytes.fromhex(l.strip()) for l in open(p) if l.strip()]
    if op == 'unpack':
        open(sys.argv[4], 'w').write(unpack(rd(sys.argv[2]), sys.argv[3]))
    elif op == 'pack':
        fr = pack(open(sys.argv[2]).read(), sys.argv[3])
        open(sys.argv[4], 'w').write(''.join(f.hex() + '\n' for f in fr))
    elif op == 'check':
        fr = rd(sys.argv[2])
        txt = unpack(fr, sys.argv[3])
        back = pack(txt, sys.argv[3])
        bad = [i for i, (a, b) in enumerate(zip(fr, back)) if a != b]
        nunk = sum(1 for l in txt.splitlines() if l.startswith('unknown:'))
        print('IDENTICAL' if not bad and len(fr) == len(back) else 'DIFFER frames %s' % bad[:10],
              '| %d config lines, %d unknown bits' % (len(txt.splitlines()), nunk))
        sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
