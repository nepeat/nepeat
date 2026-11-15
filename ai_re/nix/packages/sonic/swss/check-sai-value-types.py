#!/usr/bin/env python3
"""Find SAI attribute values written/read through a union member of the wrong
width (e.g. .u32 for a sai_uint16_t attribute): correct on little-endian,
wrong on big-endian. Heuristic: tracks "<var>.id = SAI_..._ATTR_..." and
"case SAI_..._ATTR_...:" and checks later "<var>.value.<int member>" uses.
False positives where .value is set before .id; misses ids set indirectly.
usage: check-sai-value-types.py SAI_INCLUDE_DIR SRC_DIR...
"""
import glob, os, re, sys

WIDTH = {'u8': 1, 's8': 1, 'u16': 2, 's16': 2, 'u32': 4, 's32': 4, 'u64': 8, 's64': 8}
TYPE_WIDTH = {'sai_uint8_t': 1, 'sai_int8_t': 1, 'sai_cos_t': 1, 'sai_queue_index_t': 1,
              'sai_uint16_t': 2, 'sai_int16_t': 2, 'sai_vlan_id_t': 2,
              'sai_uint32_t': 4, 'sai_int32_t': 4, 'sai_label_id_t': 4, 'sai_ip4_t': 4,
              'sai_uint64_t': 8, 'sai_int64_t': 8}

def attr_types(inc):
    types = {}
    for h in glob.glob(os.path.join(inc, 'sai*.h')):
        t = None
        for line in open(h, errors='replace'):
            m = re.search(r'@type\s+(\S+)', line)
            if m:
                t = m.group(1)
                continue
            m = re.match(r'\s*(SAI_\w+_ATTR_\w+)\s*(=[^,]*)?,', line)
            if m and t:
                types[m.group(1)] = t
                t = None
    return types

def width(t):
    if t in TYPE_WIDTH:
        return TYPE_WIDTH[t]
    if re.fullmatch(r'sai_\w+_t', t) and 'list' not in t and t not in (
            'sai_object_id_t', 'sai_mac_t', 'sai_ip_address_t', 'sai_ip6_t', 'sai_pointer_t'):
        return 4   # enum
    return None

def main():
    types = attr_types(sys.argv[1])
    for d in sys.argv[2:]:
        files = glob.glob(d + '/**/*.cpp', recursive=True) + glob.glob(d + '/**/*.h', recursive=True)
        for f in sorted(x for x in files if '/tests/' not in x):
            cur, case = {}, None
            for n, line in enumerate(open(f, errors='replace'), 1):
                for m in re.finditer(r'([\w.\[\]>-]+?)\.id\s*=\s*(SAI_\w+_ATTR_\w+)', line):
                    cur[m.group(1)] = m.group(2)
                m = re.search(r'case\s+(SAI_\w+_ATTR_\w+)\s*:', line)
                if m:
                    case = m.group(1)
                for m in re.finditer(r'([\w.\[\]>-]+?)\.value\.(u8|s8|u16|s16|u32|s32|u64|s64)\b', line):
                    a = cur.get(m.group(1)) or case
                    t = types.get(a or '')
                    w = width(t) if t else None
                    if w and w != WIDTH[m.group(2)]:
                        print('%s:%d: %s is %s, used .%s' % (f, n, a, t, m.group(2)))

if __name__ == '__main__':
    main()
