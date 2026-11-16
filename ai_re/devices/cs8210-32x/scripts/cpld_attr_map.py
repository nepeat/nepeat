#!/usr/bin/env python3
"""Map every sysfs attribute of the vendor sys_cpld.ko (Celestica 'celestica-cpld',
v1.7) to the CPLD register bits it reads and writes, by emulating the module's
show/store functions in Unicorn with a fake register cache. Static: no hardware.

usage: cpld_attr_map.py sys_cpld.ko [--json OUT]
Needs pyelftools + unicorn (ai_re devshell python).
"""
import argparse, json, struct, sys
from elftools.elf.elffile import ELFFile
from elftools.elf.relocation import RelocationSection
from unicorn import Uc, UC_ARCH_ARM64, UC_MODE_ARM, UC_HOOK_CODE, UcError
from unicorn.arm64_const import *

MAPBASE = 0x100000
SECBASE = {}
STACK, STOP, HEAP = 0xb00000, 0x8ff000, 0xc00000
DEV, ATTR, PRIV, CACHE, BUF = HEAP + 0x100, HEAP + 0x1000, HEAP + 0x2000, HEAP + 0x3000, HEAP + 0x4000


def load(path):
    e = ELFFile(open(path, 'rb'))
    secs = {}
    addr = MAPBASE
    for i, s in enumerate(e.iter_sections()):
        if s['sh_flags'] & 2 and s['sh_size']:  # SHF_ALLOC
            secs[i] = (s.name, addr, bytearray(s.data()) if s['sh_type'] != 'SHT_NOBITS' else bytearray(s['sh_size']))
            addr += (s['sh_size'] + 0xfff) & ~0xfff
    symtab = e.get_section_by_name('.symtab')
    syms = list(symtab.iter_symbols())
    def symaddr(sym):
        if sym['st_shndx'] in ('SHN_UNDEF', 'SHN_ABS') or not isinstance(sym['st_shndx'], int) or sym['st_shndx'] not in secs:
            return None
        return secs[sym['st_shndx']][1] + sym['st_value']
    externs = {}
    nextext = [0x800000]
    calls = {}
    for rs in e.iter_sections():
        if not isinstance(rs, RelocationSection):
            continue
        tgt = rs['sh_info']
        if tgt not in secs:
            continue
        name, base, buf = secs[tgt]
        st = e.get_section(rs['sh_link'])
        for r in rs.iter_relocations():
            sym = st.get_symbol(r['r_info_sym'])
            S = symaddr(sym)
            if S is None:
                if sym.name not in externs:
                    externs[sym.name] = nextext[0]; nextext[0] += 0x10
                S = externs[sym.name]
            A = r['r_addend']; P = base + r['r_offset']; off = r['r_offset']; t = r['r_info_type']
            V = S + A
            if t == 257:  # ABS64
                struct.pack_into('<Q', buf, off, V)
            elif t == 275:  # ADR_PREL_PG_HI21
                ins = struct.unpack_from('<I', buf, off)[0]
                imm = ((V & ~0xfff) - (P & ~0xfff)) >> 12
                ins = (ins & 0x9f00001f) | ((imm & 3) << 29) | (((imm >> 2) & 0x7ffff) << 5)
                struct.pack_into('<I', buf, off, ins)
            elif t in (277, 278, 284, 285, 286, 299):  # ADD/LDST8/16/32/64/128 LO12
                ins = struct.unpack_from('<I', buf, off)[0]
                lo = V & 0xfff
                sh = {277: 0, 278: 0, 284: 1, 285: 2, 286: 3, 299: 4}[t]
                ins = (ins & ~(0xfff << 10)) | (((lo >> sh) & 0xfff) << 10)
                struct.pack_into('<I', buf, off, ins)
            elif t in (283, 282):  # CALL26 / JUMP26
                if sym.name in externs or symaddr(sym) is None:
                    calls[P] = sym.name
                else:
                    ins = struct.unpack_from('<I', buf, off)[0]
                    imm = ((V - P) >> 2) & 0x3ffffff
                    struct.pack_into('<I', buf, off, (ins & 0xfc000000) | imm)
            elif t in (261, 260):  # PREL32/PREL16 (eh_frame etc.)
                pass
    funcs = {s.name: symaddr(s) for s in syms if s['st_info']['type'] in ('STT_FUNC', 'STT_OBJECT') and symaddr(s) is not None}
    return e, secs, syms, funcs, calls


class Emu:
    def __init__(self, path):
        self.e, self.secs, self.syms, self.funcs, self.calls = load(path)
        self.uc = Uc(UC_ARCH_ARM64, UC_MODE_ARM)
        self.uc.mem_map(MAPBASE, 0x700000)
        self.uc.mem_map(0x800000, 0x100000)
        self.uc.mem_map(STACK - 0x80000, 0x90000)
        self.uc.mem_map(HEAP, 0x100000)
        for name, base, buf in self.secs.values():
            self.uc.mem_write(base, bytes(buf))
        self.data0 = {base: bytes(buf) for name, base, buf in self.secs.values() if name.startswith('.data')}
        self.uc.hook_add(UC_HOOK_CODE, self.hook, begin=0x800000, end=0x8fffff)
        self.uc.hook_add(UC_HOOK_CODE, self.hookc)

    def hookc(self, uc, addr, size, _):
        name = self.calls.get(addr)
        if name:
            self.ext(uc, name)
            uc.reg_write(UC_ARM64_REG_PC, addr + 4)
        elif addr == self.funcs.get('cpld_update_device'):
            uc.reg_write(UC_ARM64_REG_X0, PRIV)
            uc.reg_write(UC_ARM64_REG_PC, uc.reg_read(UC_ARM64_REG_X30))

    def hook(self, uc, addr, size, _):
        pass

    def ext(self, uc, name):
        x = [uc.reg_read(r) for r in (UC_ARM64_REG_X0, UC_ARM64_REG_X1, UC_ARM64_REG_X2, UC_ARM64_REG_X3, UC_ARM64_REG_X4)]
        ret = 0
        if name == 'sprintf':
            self.out.append(('sprintf', x[2], x[3]))
            ret = 2
        elif name in ('kstrtoint', 'kstrtou8', 'kstrtoll', 'kstrtouint', 'kstrtol', 'kstrtoul'):
            sz = {'kstrtoint': '<i', 'kstrtou8': '<B', 'kstrtoll': '<q', 'kstrtouint': '<I', 'kstrtol': '<q', 'kstrtoul': '<Q'}[name]
            uc.mem_write(x[2], struct.pack(sz, self.value & ((1 << (struct.calcsize(sz) * 8)) - 1) if sz in ('<B', '<I', '<Q') else self.value))
        elif name == 'i2c_smbus_write_byte_data':
            self.writes.append((x[1] & 0xff, x[2] & 0xff))
            uc.mem_write(CACHE + (x[1] & 0xff), bytes([x[2] & 0xff]))
            self.cache[x[1] & 0xff] = x[2] & 0xff
        elif name == 'i2c_smbus_read_byte_data':
            self.reads.append(x[1] & 0xff)
            ret = self.cache[x[1] & 0xff]
        elif name in ('strcmp', 'strncmp'):
            a = bytes(uc.mem_read(x[0], 32)).split(b'\0')[0]; b = bytes(uc.mem_read(x[1], 32)).split(b'\0')[0]
            ret = 0 if a == b else 1
        elif name in ('msleep', 'udelay', '_mcount', 'mutex_lock', 'mutex_unlock', 'dev_err', '_dev_info', '__dynamic_dev_dbg', 'printk', 'usleep_range'):
            pass
        else:
            self.unhandled.add(name)
        uc.reg_write(UC_ARM64_REG_X0, ret & 0xffffffffffffffff)

    def run(self, fn, index, cache, value=None, buf=b''):
        uc = self.uc
        for base, d in self.data0.items():
            uc.mem_write(base, d)
        self.cache = bytearray(cache)
        uc.mem_write(CACHE, bytes(cache))
        uc.mem_write(PRIV + 72, struct.pack('<Q', CACHE))
        uc.mem_write(PRIV + 80, struct.pack('<i', 255))
        uc.mem_write(ATTR + 32, struct.pack('<i', index))
        uc.mem_write(DEV + 160, struct.pack('<Q', PRIV))
        uc.mem_write(BUF, buf + b'\0' * 64)
        self.out, self.writes, self.reads, self.unhandled = [], [], [], set()
        self.value = value or 0
        uc.reg_write(UC_ARM64_REG_SP, STACK)
        uc.reg_write(UC_ARM64_REG_X30, STOP)
        uc.reg_write(UC_ARM64_REG_X0, DEV)
        uc.reg_write(UC_ARM64_REG_X1, ATTR)
        uc.reg_write(UC_ARM64_REG_X2, BUF)
        uc.reg_write(UC_ARM64_REG_X3, len(buf))
        try:
            uc.emu_start(self.funcs[fn], STOP, count=50000)
        except UcError as ex:
            return {'error': str(ex), 'pc': hex(uc.reg_read(UC_ARM64_REG_PC))}
        r = uc.reg_read(UC_ARM64_REG_X0)
        if r >= 1 << 63:
            r -= 1 << 64
        return {'ret': r, 'out': list(self.out), 'writes': list(self.writes), 'reads': list(self.reads), 'unhandled': sorted(self.unhandled)}


def getter_bits(em, fn, idx):
    base = em.run(fn, idx, bytes(256))
    if 'error' in base:
        return {'error': base}
    v0 = base['out'][0][1] if base['out'] else None
    deps = []
    for r in range(256):
        c = bytearray(256); c[r] = 0xff
        res = em.run(fn, idx, bytes(c))
        v = res['out'][0][1] if res.get('out') else None
        if v != v0:
            bits = []
            for b in range(8):
                c = bytearray(256); c[r] = 1 << b
                res = em.run(fn, idx, bytes(c))
                if (res['out'][0][1] if res.get('out') else None) != v0:
                    bits.append(b)
            deps.append({'reg': '0x%02x' % r, 'bits': bits})
    return {'deps': deps, 'zero_value': v0, 'fmt_ret': base['ret']}


def setter(em, fn, idx, values):
    out = {}
    for v in values:
        row = {}
        for fill in (0x00, 0xff):
            res = em.run(fn, idx, bytes([fill]) * 256, value=v, buf=str(v).encode())
            if 'error' in res:
                row['fill%02x' % fill] = res
            else:
                row['fill%02x' % fill] = {'writes': ['0x%02x=0x%02x' % w for w in res['writes']], 'ret': res['ret'], 'unhandled': res['unhandled']}
        out[str(v)] = row
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('ko'); ap.add_argument('--attrs', required=True, help='JSON from elfattrs (name/show/store/index)')
    ap.add_argument('--json')
    a = ap.parse_args()
    em = Emu(a.ko)
    attrs = json.load(open(a.attrs))
    res = []
    for at in attrs:
        item = {'name': at['name'], 'mode': at['mode'], 'index': at['index']}
        if at['show'] in ('cpld_get_data', 'get_led') and at['index'] is not None:
            item['read'] = getter_bits(em, at['show'], at['index'])
        if at['store'] in ('cpld_set_data', 'set_led') and at['index'] is not None:
            item['write'] = setter(em, at['store'], at['index'], [0, 1, 2, 3, 7] if at['store'] == 'set_led' else [0, 1])
        res.append(item)
    js = json.dumps(res, indent=1)
    if a.json:
        open(a.json, 'w').write(js + '\n')
    print(js)


if __name__ == '__main__':
    main()
