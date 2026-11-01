#!/usr/bin/env python3
"""Emulate two captured ARM64 modules' setters with I2C replaced by Python.

No device access. This validates binary behavior, not actual CPLD electrical
behavior. The sys-CPLD switch-table offsets are specific to the captured module.
"""
import hashlib
import json
from pathlib import Path
import struct
from elftools.elf.elffile import ELFFile
from elftools.elf.relocation import RelocationSection
from unicorn import Uc, UC_ARCH_ARM64, UC_MODE_ARM, UC_HOOK_CODE
from unicorn.arm64_const import UC_ARM64_REG_X0, UC_ARM64_REG_X1, UC_ARM64_REG_X2, UC_ARM64_REG_X3, UC_ARM64_REG_X30, UC_ARM64_REG_SP, UC_ARM64_REG_PC

BASE, RO, HEAP, STACK, STOP = 0x100000, 0x200000, 0x300000, 0x400000, 0x4f0000
DEV, ATTR, PRIV, DATA = HEAP + 0x100, HEAP + 0x1000, HEAP + 0x2000, HEAP + 0x3000


def emulate(path, attribute, value):
    expected = {'sys_cpld.ko': '232725645895cf4b6b393342f508d23e4f29286ebfbaf8a9fb8d16afdeecb72f', 'fan_cpld.ko': 'b5a7d5542e3fb1ebff371c4051cfc570b023f403d4b2f600ee696a5483746b7c'}
    assert hashlib.sha256(path.read_bytes()).hexdigest() == expected[path.name], "unreviewed module version"
    with path.open('rb') as stream:
        elf = ELFFile(stream)
        symbols = {s.name: s for s in elf.get_section_by_name('.symtab').iter_symbols()}
        text = elf.get_section_by_name('.text').data()
        rodata = elf.get_section_by_name('.rodata').data()
        attr = symbols['sensor_dev_attr_' + attribute]
        section = elf.get_section(attr['st_shndx']).data()
        index = struct.unpack_from('<i', section, attr['st_value'] + 32)[0]
        calls = {}
        for sec in elf.iter_sections():
            if isinstance(sec, RelocationSection) and elf.get_section(sec['sh_info']).name == '.text':
                table = elf.get_section(sec['sh_link'])
                for rel in sec.iter_relocations():
                    if rel['r_info_type'] == 283:  # R_AARCH64_CALL26
                        calls[BASE + rel['r_offset']] = table.get_symbol(rel['r_info_sym']).name
        is_system = path.stem == 'sys_cpld'
        function = 'cpld_set_data' if is_system else 'fan_set_watchdog'
        start = BASE + symbols[function]['st_value']
        stubs = {BASE + symbols[n]['st_value']: n for n in (
            ['cpld_update_device'] if is_system else ['fan_cpld_i2c_read8', 'fan_cpld_i2c_write8'])}
    uc = Uc(UC_ARCH_ARM64, UC_MODE_ARM)
    uc.mem_map(BASE, 0x400000)
    uc.mem_write(BASE, text)
    uc.mem_write(RO, rodata)
    uc.mem_write(ATTR + 32, struct.pack('<i', index))
    uc.mem_write(PRIV + 72, struct.pack('<Q', DATA))
    uc.mem_write(DATA, b'\xa5' * 512)  # preserve unrelated bits, not a reset default
    uc.reg_write(UC_ARM64_REG_SP, STACK + 0x8000)
    uc.reg_write(UC_ARM64_REG_X30, STOP)
    for reg, val in [(UC_ARM64_REG_X0, DEV), (UC_ARM64_REG_X1, ATTR),
                     (UC_ARM64_REG_X2, HEAP + 0x4000), (UC_ARM64_REG_X3, len(str(value)))]:
        uc.reg_write(reg, val)
    writes = []
    reads = []

    def hook(emu, addr, size, _):
        if is_system and addr == BASE + 0xf9c:
            # Resolve the captured ADRP+ADD relocation to the switch table.
            emu.reg_write(UC_ARM64_REG_X0, RO + 0x270)
            emu.reg_write(UC_ARM64_REG_PC, BASE + 0xfa4)
            return
        name = calls.get(addr) or stubs.get(addr)
        if not name:
            return
        ret = 0
        if name == 'cpld_update_device':
            ret = PRIV
        elif name in ('kstrtoint', 'kstrtoll'):
            target = emu.reg_read(UC_ARM64_REG_X2)
            emu.mem_write(target, struct.pack('<i' if name == 'kstrtoint' else '<q', value))
        elif name in ('i2c_smbus_write_byte_data', 'fan_cpld_i2c_write8'):
            reg = emu.reg_read(UC_ARM64_REG_X1) & 0xff
            val = emu.reg_read(UC_ARM64_REG_X2) & 0xff
            writes.append({'register': hex(reg), 'value': hex(val)})
            emu.mem_write(DATA + reg, bytes([val]))
        elif name == 'fan_cpld_i2c_read8':
            reg = emu.reg_read(UC_ARM64_REG_X1) & 0xff
            ret = emu.mem_read(DATA + reg, 1)[0]
            reads.append(hex(reg))
        elif name not in ('_mcount', 'mutex_lock', 'mutex_unlock', 'dev_err'):
            raise RuntimeError(f'unhandled call {name} at {addr:x}')
        emu.reg_write(UC_ARM64_REG_X0, ret)
        emu.reg_write(UC_ARM64_REG_PC, emu.reg_read(UC_ARM64_REG_X30) if addr in stubs else addr + 4)

    uc.hook_add(UC_HOOK_CODE, hook)
    uc.emu_start(start, STOP, count=20000)
    assert uc.reg_read(UC_ARM64_REG_PC) == STOP, 'instruction limit reached'
    ret = uc.reg_read(UC_ARM64_REG_X0)
    if ret >= (1 << 63):
        ret -= 1 << 64
    return {'module': path.name, 'attribute': attribute, 'input': value,
            'attribute_index': index, 'initial_register_bytes': '0xa5',
            'reads': reads, 'writes': writes, 'return': ret}


def main():
    project = Path(__file__).resolve().parents[1]
    driver = project / 'analysis/extracted/root/driver'
    cases = [('sys_cpld', a, v) for a in ['fan_max_speed', 'fan2_max_speed', 'system_watchdog_enable'] for v in [0, 1]]
    cases += [('sys_cpld', 'system_watchdog_seconds', v) for v in [0, 30, 200, 201]]
    cases += [('fan_cpld', 'fan_watchdog_enable', v) for v in [0, 1]]
    cases += [('fan_cpld', 'fan_watchdog_seconds', v) for v in [0, 30, 635, 636]]
    result = [emulate(driver / (module + '.ko'), attr, value) for module, attr, value in cases]
    report = {'method': __doc__, 'modules': {n: hashlib.sha256((driver / (n + '.ko')).read_bytes()).hexdigest()
              for n in ['sys_cpld', 'fan_cpld']}, 'traces': result}
    out = project / 'analysis/research/cpld-setter-traces.json'
    out.write_text(json.dumps(report, indent=2) + '\n')
    for item in result:
        print(item['module'], item['attribute'], item['input'], item['writes'], 'return', item['return'])


if __name__ == '__main__':
    main()
