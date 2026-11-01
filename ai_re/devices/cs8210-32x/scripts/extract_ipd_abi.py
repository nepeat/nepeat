#!/usr/bin/env python3
"""Recover SDK/kernel ABI layouts from ipd.ko DWARF (no module execution)."""
import hashlib
import json
from pathlib import Path
import struct
from elftools.elf.elffile import ELFFile


def value(die, attr, default=None):
    a = die.attributes.get(attr)
    if a is None:
        return default
    return a.value.decode(errors='replace') if isinstance(a.value, bytes) else a.value


def describe(die, depth=0):
    if die is None:
        return {'name': 'void'}
    result = {'tag': die.tag, 'name': value(die, 'DW_AT_name'), 'bytes': value(die, 'DW_AT_byte_size')}
    if depth > 7:
        return result
    if 'DW_AT_type' in die.attributes:
        result['type'] = describe(die.get_DIE_from_attribute('DW_AT_type'), depth + 1)
    if die.tag in ('DW_TAG_structure_type', 'DW_TAG_union_type'):
        result['members'] = [{'name': value(m, 'DW_AT_name'),
                              'offset': value(m, 'DW_AT_data_member_location', 0),
                              'type': describe(m.get_DIE_from_attribute('DW_AT_type'), depth + 1)}
                             for m in die.iter_children() if m.tag == 'DW_TAG_member']
    elif die.tag == 'DW_TAG_array_type':
        result['dimensions'] = [value(m, 'DW_AT_upper_bound') for m in die.iter_children()
                                if m.tag == 'DW_TAG_subrange_type']
    elif die.tag == 'DW_TAG_enumeration_type':
        result['values'] = {value(m, 'DW_AT_name'): value(m, 'DW_AT_const_value') for m in die.iter_children()}
    return result


def main():
    project = Path(__file__).resolve().parents[1]
    path = project / 'analysis/extracted/root/sdk/R3181-J0001-01_V0.1.2_Briggs_SDK/ipd.ko'
    result = {'module_sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'types': {}}
    with path.open('rb') as stream:
        elf = ELFFile(stream)
        dwarf = elf.get_dwarf_info()
        for cu in dwarf.iter_CUs():
            for die in cu.iter_DIEs():
                name = value(die, 'DW_AT_name', '')
                if die.tag == 'DW_TAG_typedef' and (name.startswith('inno_ioctl_') or name in ('inno_dma_alloc_t', 'inno_dev_info_t')):
                    result['types'].setdefault(name, describe(die))
        # Captured inno_ioctl masks cmd to its low byte, subtracts one, bounds
        # checks <= 25, then uses this signed-halfword switch table.
        rodata = elf.get_section_by_name('.rodata').data()
        result['dispatch_low_byte'] = {n: hex(0x90a8 + 4 * struct.unpack_from('<h', rodata, 0x4d0 + 2 * (n - 1))[0])
                                       for n in range(1, 27)}
    out = project / 'analysis/research/ipd-abi.json'
    out.write_text(json.dumps(result, indent=2) + '\n')
    for name, info in result['types'].items():
        target = info['type']
        print(name, target.get('bytes'), [(m['name'], m['offset']) for m in target.get('members', [])])


if __name__ == '__main__':
    main()
