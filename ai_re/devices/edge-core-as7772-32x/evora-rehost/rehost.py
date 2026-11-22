#!/usr/bin/env python3
"""Re-host the vendor Evora (BCM82391) PHY library from the AS7772 diag bcm.user (ppc32, static) under Unicorn.

The vendor binary is read at run time (never copied). glibc start-up runs for real up to main(); main() is not
run. The diag 'extphy' sub-command handlers are then called directly with (argc, argv). The two MDIO callbacks
the diag passes to the library are replaced by a Python backend (mock or the real PHYs through our bcm.user).

  rehost.py [--backend mock|ssh] [--trace FILE] 'init 0x00 4' 'cfg 0x00 nphy=4 speed=100000 ...' 'status 0x00'
"""
import argparse, os, struct, sys, time
from unicorn import *
from unicorn.ppc_const import *
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mdio

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_BIN = os.path.join(HERE, '..', 'dumps/x/diag/root/usr/local/sbin/bcm.user')
BIN_SHA256_PREFIX = 'e163e815'

# addresses in the vendor binary (see ../evora-vendor-re.md)
MDIO_READ, MDIO_WRITE = 0x11737db0, 0x11737e20
SUBCMD = {'init': 0x117417f0, 'cfg': 0x11741a40, 'pre': 0x11741c80, 'prbs': 0x11742010, 'eyescan': 0x11742270,
          'status': 0x11742860, 'info': 0x11743000, 'reg': 0x117431d0, 'reg64': 0x11743660, 'lba': 0x117416e0,
          'power': 0x117430b0, 'ver': 0x11742430, 'polarity': 0x11741e20}
# never execute: SPI EEPROM programming is selected by the firmware-load method, checked in forbid()
DOWNLOAD_FN = 0x11835cf0
MAGIC = 0x0fff0000
STACK_TOP = 0x7ff00000
BRK0 = 0x13400000
MMAP0 = 0x60000000
R = lambda n: UC_PPC_REG_0 + n


def _memcpy(e, d, s, n):
    e.uc.mem_write(d, bytes(e.uc.mem_read(s, n)))


def _memset(e, d, c, n):
    e.uc.mem_write(d, bytes([c & 0xff]) * n)


# vendor libc routines that use instructions the emulated CPU lacks -> Python (return None = return arg 0)
LIBC_HOOKS = {0x119164a0: _memcpy, 0x11915e64: _memset}


class Stop(Exception):
    pass


class Emu:
    def __init__(self, a):
        self.a = a
        self.raw = open(a.bin, 'rb').read()
        self.uc = uc = Uc(UC_ARCH_PPC, UC_MODE_PPC32 | UC_MODE_BIG_ENDIAN)
        # The binary is ppc32 code built for the e6500 (64-bit core): compiler output uses isel, and the hand-written
        # glibc string routines use 64-bit ld/std and AltiVec. Unicorn's ppc64 models do not work, so run an e500mc
        # (has isel; needs the virtual TLB) and replace those libc routines with Python (LIBC_HOOKS).
        uc.ctl_set_cpu_model(UC_CPU_PPC32_E500MC)
        uc.ctl_set_tlb_mode(UC_TLB_VIRTUAL) if hasattr(uc, 'ctl_set_tlb_mode') else uc.ctl_tlb_mode(UC_TLB_VIRTUAL)  # the binary uses isel (e500mc/e6500 userland)
        self.trace = mdio.Trace(a.trace)
        self.be = mdio.make_backend(a.backend, a)
        self.vclock = 1_700_000_000_000_000  # virtual microseconds
        self.out = b''
        self.err = None
        self.brk = BRK0
        self.mmap_next = MMAP0
        self.nsys = 0
        self.last = {}
        self.load()
        uc.hook_add(UC_HOOK_INTR, self.on_intr)
        uc.hook_add(UC_HOOK_CODE, self.on_mdio_read, begin=MDIO_READ, end=MDIO_READ)
        uc.hook_add(UC_HOOK_CODE, self.on_mdio_write, begin=MDIO_WRITE, end=MDIO_WRITE)
        uc.hook_add(UC_HOOK_MEM_UNMAPPED, self.on_unmapped)
        uc.hook_add(UC_HOOK_CODE, self.on_download, begin=DOWNLOAD_FN, end=DOWNLOAD_FN)
        for addr, fn in LIBC_HOOKS.items():
            uc.hook_add(UC_HOOK_CODE, self.on_libc, fn, begin=addr, end=addr)
        if a.itrace:
            self.itf = open(a.itrace, 'w')
            uc.hook_add(UC_HOOK_BLOCK, lambda u, addr, sz, d: self.itf.write('%08x\n' % addr))

    # ---- loading -------------------------------------------------------------------------------------
    def load(self):
        uc, raw = self.uc, self.raw
        assert raw[:4] == b'\x7fELF' and raw[4] == 1 and raw[5] == 2
        phoff, = struct.unpack('>I', raw[28:32])
        phentsize, phnum = struct.unpack('>HH', raw[42:46])
        self.entry, = struct.unpack('>I', raw[24:28])
        uc.mem_map(0x10000000, BRK0 - 0x10000000)
        uc.mem_map(BRK0, 0x10000000)           # brk heap
        uc.mem_map(MMAP0, 0x10000000)          # anonymous mmap arena
        uc.mem_map(STACK_TOP - 0x1000000, 0x1001000)
        uc.mem_map(MAGIC, 0x1000)
        uc.mem_write(MAGIC, struct.pack('>I', 0x48000000) * 4)
        self.phdr = None
        for i in range(phnum):
            p_type, p_off, p_va, _, p_fsz, p_msz, _, _ = struct.unpack('>8I', raw[phoff + i * phentsize: phoff + i * phentsize + 32])
            if p_type == 1:
                uc.mem_write(p_va, raw[p_off:p_off + p_fsz])
                if p_off == 0:
                    self.phdr = p_va + phoff
        self.phnum, self.phent = phnum, phentsize
        self.scratch = MMAP0 + 0x0f000000  # argv strings etc.

    def rr(self, r):
        return self.uc.reg_read(r) & 0xffffffff

    def u32(self, a):
        return struct.unpack('>I', self.uc.mem_read(a, 4))[0]

    def cstr(self, a, n=4096):
        b = bytes(self.uc.mem_read(a, n))
        return b[:b.index(0)] if 0 in b else b

    def alloc(self, data):
        a = self.scratch
        self.uc.mem_write(a, data)
        self.scratch += (len(data) + 15) & ~15
        return a

    # ---- libc start-up -------------------------------------------------------------------------------
    def boot(self):
        """Run _start/__libc_start_main up to (not into) main()."""
        uc = self.uc
        # _start: lis r8,hi ; lwzu r13,lo(r8) -> r8 = &{sda_base, main, init, fini}
        w0, w1 = struct.unpack('>II', uc.mem_read(self.entry + 0x18, 8))
        assert w0 >> 16 == 0x3d00 and w1 >> 16 == 0x85a8, 'unexpected _start'
        lo = w1 & 0xffff
        tbl = ((w0 & 0xffff) << 16) + (lo - 0x10000 if lo & 0x8000 else lo)
        self.main = self.u32(tbl + 4)
        sp = STACK_TOP - 0x1000
        strs = sp + 0x100
        uc.mem_write(strs, b'bcm.user\0' + b'\0' * 7 + bytes(range(16)))
        aux = [(3, self.phdr), (4, self.phent), (5, self.phnum), (6, 4096), (9, self.entry), (11, 0), (12, 0),
               (13, 0), (14, 0), (16, 0), (17, 100), (19, 32), (20, 32), (21, 0), (23, 0), (25, strs + 16), (0, 0)]
        words = [1, strs, 0, 0] + [x for kv in aux for x in kv]
        uc.mem_write(sp, struct.pack('>%dI' % len(words), *words))
        uc.reg_write(R(1), sp)
        uc.reg_write(UC_PPC_REG_MSR, uc.reg_read(UC_PPC_REG_MSR) | 0x2000)  # FP available
        self.run(self.entry, self.main, 'libc start-up')

    def run(self, pc, until, what):
        try:
            self.uc.emu_start(pc, until)
        except UcError as e:
            self.report('%s: emulator error %s' % (what, e))
            raise
        if self.err:
            e, self.err = self.err, None
            self.report('%s: %s' % (what, e))
            raise e

    def report(self, msg):
        uc = self.uc
        sys.stderr.write('!! %s\n   pc=%08x lr=%08x sp=%08x r3=%08x r4=%08x r5=%08x\n' % (
            msg, self.rr(UC_PPC_REG_PC), self.rr(UC_PPC_REG_LR), self.rr(R(1)),
            self.rr(R(3)), self.rr(R(4)), self.rr(R(5))))
        sys.stderr.write('   backtrace: ' + ' '.join('%08x' % x for x in self.backtrace()) + '\n')

    def backtrace(self, n=24):
        uc = self.uc
        out = [self.rr(UC_PPC_REG_LR)]
        sp = self.rr(R(1))
        try:
            for _ in range(n):
                sp = self.u32(sp)
                if not sp:
                    break
                out.append(self.u32(sp + 4))
        except UcError:
            pass
        return out

    def call(self, fn, *args):
        uc = self.uc
        for i, v in enumerate(args):
            uc.reg_write(R(3 + i), v & 0xffffffff)
        uc.reg_write(UC_PPC_REG_LR, MAGIC)
        self.run(fn, MAGIC, 'call %08x' % fn)
        return self.rr(R(3))

    def extphy(self, line):
        argv = line.split()
        if argv[0] == 'extphy':
            argv = argv[1:]
        fn = SUBCMD[argv[0]]
        for x in argv:
            if x.startswith('fw_load=') and x not in ('fw_load=0', 'fw_load=1', 'fw_load=2'):
                raise Stop('refusing %s: only RAM download methods 0/1/2 (12 selects SPI EEPROM programming)' % x)
        rest = argv[1:]
        ptrs = [self.alloc(s.encode() + b'\0') for s in rest] + [0]
        av = self.alloc(struct.pack('>%dI' % len(ptrs), *ptrs))
        self.trace.note('extphy ' + ' '.join(argv))
        self.say('>>> extphy %s\n' % ' '.join(argv))
        t0 = time.time(); n0 = dict(self.trace.n)
        r = self.call(fn, len(rest), av)
        if hasattr(self.be, 'flush'):
            self.be.flush()
        r = r - (1 << 32) if r & 0x80000000 else r
        d = {k: self.trace.n[k] - n0.get(k, 0) for k in self.trace.n}
        self.say('<<< rc=%d  (%d reads, %d writes, %d delays, %.1f s)\n' % (r, d['R'], d['W'], d['D'], time.time() - t0))
        self.trace.note('rc=%d' % r)
        return r

    # ---- state: the library keeps per-PHY state in RAM (firmware loaded, MACsec devices); status/cfg need it -------
    def regions(self):
        return [(0x1238e000, BRK0 - 0x1238e000), (BRK0, ((self.brk + 0xfff) & ~0xfff) - BRK0),
                (MMAP0, self.mmap_next - MMAP0), (STACK_TOP - 0x10000, 0x10000)]

    def save_state(self, path):
        import pickle, zlib
        st = {'brk': self.brk, 'mmap_next': self.mmap_next, 'vclock': self.vclock, 'scratch': self.scratch,
              'mem': [(a, zlib.compress(bytes(self.uc.mem_read(a, n)), 1)) for a, n in self.regions() if n]}
        with open(path, 'wb') as f:
            pickle.dump(st, f)

    def load_state(self, path):
        import pickle, zlib
        with open(path, 'rb') as f:
            st = pickle.load(f)
        self.brk, self.mmap_next, self.vclock, self.scratch = st['brk'], st['mmap_next'], st['vclock'], st['scratch']
        for a, z in st['mem']:
            self.uc.mem_write(a, zlib.decompress(z))

    def say(self, s):
        sys.stdout.write(s); sys.stdout.flush()
        if self.a.log:
            with open(self.a.log, 'a') as f:
                f.write(s)

    # ---- hooks ---------------------------------------------------------------------------------------
    def ret(self, val):
        uc = self.uc
        uc.reg_write(R(3), val & 0xffffffff)
        uc.reg_write(UC_PPC_REG_PC, self.rr(UC_PPC_REG_LR))

    def fail(self, e):
        self.err = e
        self.uc.emu_stop()

    def on_mdio_read(self, uc, addr, size, ud):
        try:
            phy, reg, ptr = self.rr(R(4)) & 0xffff, self.rr(R(5)), self.rr(R(6))
            devad, r16 = (reg >> 16) & 0x1f, reg & 0xffff
            v = self.be.read(phy, devad, r16) & 0xffff
            self.trace.ev('R', phy, devad, r16, v)
            self.last[(phy, r16)] = v
            if self.a.verbose:
                self.say('  R %03x %x.%04x = %04x   [%s]\n' % (phy, devad, r16, v, ' '.join('%08x' % x for x in self.backtrace(4))))
            uc.mem_write(ptr, struct.pack('>I', v))
            self.ret(0)
        except Exception as e:
            self.fail(e)

    def on_mdio_write(self, uc, addr, size, ud):
        try:
            phy, reg, val = self.rr(R(4)) & 0xffff, self.rr(R(5)), self.rr(R(6)) & 0xffff
            devad, r16 = (reg >> 16) & 0x1f, reg & 0xffff
            self.forbid(phy, devad, r16, val)
            self.be.write(phy, devad, r16, val)
            self.trace.ev('W', phy, devad, r16, val)
            if self.a.verbose:
                self.say('  W %03x %x.%04x = %04x   [%s]\n' % (phy, devad, r16, val, ' '.join('%08x' % x for x in self.backtrace(4))))
            self.ret(0)
        except Exception as e:
            self.fail(e)

    def forbid(self, phy, devad, reg, val):
        # EEPROM programming branch of plp_evora_download_prog_eeprom: sets 1.8210 bit 1 and 1.8203 bit 10.
        # (1.8210 bit 1 already reads 1 after power-up, so only a 0 -> 1 change counts.)
        for r, bit in ((0x8210, 0x0002), (0x8203, 0x0400)):
            if devad == 1 and reg == r and val & bit and not self.last.get((phy, r), 0) & bit:
                raise Stop('refusing MDIO write %03x 1.%04x = %04x: sets the SPI EEPROM programming bit' % (phy, reg, val))

    def on_download(self, uc, addr, size, ud):
        # plp_evora_download_prog_eeprom(..., r8 = "program the EEPROM")
        if self.rr(R(8)) & 0xff:
            self.fail(Stop('vendor code entered the firmware download with the EEPROM flag set: refused'))

    def on_libc(self, uc, addr, size, fn):
        try:
            r = fn(self, self.rr(R(3)), self.rr(R(4)), self.rr(R(5)))
            self.ret(self.rr(R(3)) if r is None else r)
        except Exception as e:
            self.fail(e)

    def on_unmapped(self, uc, access, addr, size, value, ud):
        self.err = Stop('unmapped memory access type %d at %08x' % (access, addr))
        return False

    def sleep_us(self, us):
        self.trace.ev('D', us)
        self.vclock += us
        if self.be.real_sleep:
            if hasattr(self.be, 'flush'):
                self.be.flush()
            time.sleep(us / 1e6)

    def on_intr(self, uc, intno, ud):
        if intno != 8:
            pc = self.rr(UC_PPC_REG_PC)
            return self.fail(Stop('exception %d, instruction %08x at %08x' % (intno, self.u32(pc - 4), pc - 4)))
        try:
            nr = self.rr(R(0))
            args = [self.rr(R(3 + i)) for i in range(6)]
            r = self.syscall(nr, args)
        except Exception as e:
            return self.fail(e)
        cr = self.rr(UC_PPC_REG_CR)
        if r < 0:
            uc.reg_write(R(3), -r)
            uc.reg_write(UC_PPC_REG_CR, cr | 0x10000000)
        else:
            uc.reg_write(R(3), r & 0xffffffff)
            uc.reg_write(UC_PPC_REG_CR, cr & ~0x10000000)

    def syscall(self, nr, a):
        uc = self.uc
        self.nsys += 1
        if self.a.strace:
            sys.stderr.write('sys %d(%s) lr=%08x\n' % (nr, ', '.join('%x' % x for x in a[:4]), self.rr(UC_PPC_REG_LR)))
        if nr == 4:  # write
            data = bytes(uc.mem_read(a[1], a[2]))
            if a[0] in (1, 2):
                self.say(data.decode('latin1'))
                return a[2]
            return -9
        if nr == 146:  # writev
            tot = 0
            for i in range(a[2]):
                base, ln = struct.unpack('>II', uc.mem_read(a[1] + 8 * i, 8))
                self.say(bytes(uc.mem_read(base, ln)).decode('latin1')); tot += ln
            return tot
        if nr == 45:  # brk
            if a[0] >= BRK0 and a[0] < BRK0 + 0x10000000:
                self.brk = a[0]
            return self.brk
        if nr in (90, 192):  # mmap, mmap2 (anonymous only)
            if a[4] != 0xffffffff and not (a[3] & 0x20):
                return -19
            ln = (a[1] + 0xfff) & ~0xfff
            addr = self.mmap_next
            self.mmap_next += ln
            if self.mmap_next > MMAP0 + 0x0f000000:
                raise Stop('mmap arena exhausted')
            uc.mem_write(addr, b'\0' * ln)
            return addr
        if nr == 221:  # futex: single thread; a wait never blocks
            return 0 if (a[1] & 0x7f) in (1, 3, 4, 5, 10) else -11
        if nr in (91, 125, 173, 174, 232, 205, 6, 300):  # munmap mprotect sigaction sigprocmask set_tid_address futex madvise close set_robust_list
            return 0
        if nr == 122:  # uname
            for i, s in enumerate((b'Linux', b'rehost', b'3.12.19', b'#1', b'ppc', b'')):
                uc.mem_write(a[0] + 65 * i, s.ljust(65, b'\0'))
            return 0
        if nr == 162:  # nanosleep
            s, ns = struct.unpack('>II', uc.mem_read(a[0], 8))
            self.sleep_us(s * 1000000 + ns // 1000)
            return 0
        if nr == 142:  # _newselect used as a sleep
            if a[4]:
                s, us = struct.unpack('>II', uc.mem_read(a[4], 8))
                self.sleep_us(s * 1000000 + us)
            return 0
        if nr == 78:  # gettimeofday
            self.vclock += 1
            if a[0]:
                uc.mem_write(a[0], struct.pack('>II', self.vclock // 1000000, self.vclock % 1000000))
            return 0
        if nr == 246:  # clock_gettime
            self.vclock += 1
            uc.mem_write(a[1], struct.pack('>II', self.vclock // 1000000, (self.vclock % 1000000) * 1000))
            return 0
        if nr == 13:  # time
            return self.vclock // 1000000
        if nr in (20, 207):  # getpid gettid
            return 1000
        if nr in (24, 47, 49, 50):  # get[e]uid/gid
            return 0
        if nr == 54:  # ioctl (isatty probes)
            return 0 if a[1] == 0x402c7413 else -25  # TCGETS ok: stdout is a tty, so line buffered
        if nr == 197:  # fstat64: say "character device" so stdout is line buffered
            uc.mem_write(a[1], b'\0' * 104)
            uc.mem_write(a[1] + 16, struct.pack('>I', 0o020620))
            return 0
        if nr == 190:  # ugetrlimit
            uc.mem_write(a[1], struct.pack('>II', 0x800000, 0xffffffff))
            return 0
        if nr == 85:  # readlink
            return -2
        if nr in (5, 33, 195, 106, 286):  # open access stat64 stat openat
            return -2
        if nr in (1, 234):
            raise Stop('vendor code called exit(%d)' % a[0])
        raise Stop('unhandled syscall %d(%s)' % (nr, ', '.join('%x' % x for x in a)))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('cmds', nargs='*', help="extphy sub-commands, e.g. 'init 0x00 4'")
    ap.add_argument('--bin', default=DEFAULT_BIN)
    ap.add_argument('--backend', default='mock', choices=['mock', 'ssh', 'diag'])
    ap.add_argument('--ssh', default='sudo ssh -T root@10.60.2.23', help='command that runs a shell command on the switch')
    ap.add_argument('--bridge-log', help='raw transcript of the diag shell on the switch')
    ap.add_argument('--trace', help='MDIO trace output file')
    ap.add_argument('--log', help='copy of the vendor console output')
    ap.add_argument('--load-state', help='restore library RAM saved by --save-state (same binary) before the commands')
    ap.add_argument('--save-state', help='save library RAM after the commands (holds vendor data: keep it under dumps/)')
    ap.add_argument('--stdin', action='store_true', help='then read more extphy commands from stdin')
    ap.add_argument('--verbose', '-v', action='store_true', help='print every MDIO access with the call chain')
    ap.add_argument('--strace', action='store_true')
    ap.add_argument('--itrace', help='basic-block address trace file (slow)')
    a = ap.parse_args()
    import hashlib
    h = hashlib.sha256(open(a.bin, 'rb').read()).hexdigest()
    if not h.startswith(BIN_SHA256_PREFIX):
        raise SystemExit('unexpected vendor binary (sha256 %s): the addresses in this tool are for %s...' % (h, BIN_SHA256_PREFIX))
    e = Emu(a)
    rc = 0
    try:
        e.boot()
        if a.load_state:
            e.load_state(a.load_state)
        for c in a.cmds:
            e.extphy(c)
        if a.stdin:
            for c in sys.stdin:
                if c.strip():
                    e.extphy(c.strip())
        if a.save_state:
            e.save_state(a.save_state)
    except (Stop, UcError, Exception) as ex:
        sys.stderr.write('STOPPED: %r\n' % (ex,))
        if a.strace:
            import traceback; traceback.print_exc()
        rc = 1
    finally:
        try:
            e.be.close()
        finally:
            e.trace.close()
    sys.stderr.write('trace: %s; syscalls %d\n' % (e.trace.n, e.nsys))
    return rc


if __name__ == '__main__':
    sys.exit(main())
