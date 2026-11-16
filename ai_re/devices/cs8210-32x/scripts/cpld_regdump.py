#!/usr/bin/env python3
"""CS8210 CPLD register dump helper (no hardware access by itself).

uboot : print vendor-U-Boot commands that read the sys CPLD (0x60), the SW LED
        CPLD (0x64) and the MACsec LED CPLD (0x63) through the board mux tree,
        skipping read-sensitive registers in the bulk pass and reading the
        latch candidates last, one byte each.
diff  : parse a console capture of those commands and compare every byte with
        what the v13/v1-era vendor software expects (diag cpld table: compared
        bits + default; sys_cpld.ko 1.7 / U-Boot .015 meanings; known live reads).

usage: cpld_regdump.py uboot
       cpld_regdump.py diff CAPTURE.txt
"""
import re, sys

# reg: (name, mask, default, note). mask/default = the vendor diag cpld test (bits it compares).
SYS = {
    0x00: ('version low byte', 0, 0, 'v13 image = 0x13; running 0x15 (U-Boot banner)'),
    0x01: ('board rev [7:4] / version high [3:0]', 0, 0, 'expect 0x10 (board_version 1 = R0B, SDK boardID 0x01)'),
    0x02: ('reset ctrl 1', 0xff, 0xff, 'active-low resets'),
    0x03: ('reset ctrl 2', 0xff, 0xff, 'b6 i2c, b5 ASIC, b3 fan, b2 LED CPLDs (active low)'),
    0x04: ('reset ctrl 3', 0xff, 0xff, 'b5 fan2'),
    0x05: ('reset ctrl 4 / misc', 0xa0, 0xe0, 'b7 fan_max_speed (0=max), b6 sys EEPROM WP, b5 fan2_max_speed'),
    0x06: ('reset ctrl 5', 0xff, 0xff, 'b2 MDIO FPGA reset (active low)'),
    0x07: ('misc interrupt', 0, 0xc8, 'latch candidate'),
    0x08: ('AC PSU 1/2 status', 0, 0xff, 'b7 voltage fault, b6 voltage up, b5/4 psu2/1 present, b3/2 input ok, b1/0 output ok'),
    0x09: ('presence detect', 0x80, 0x00, 'b0 1 = 19-inch; live 0x05'),
    0x0b: ('ACC0/1 GPIO', 0, 0x1c, ''),
    0x0c: ('DC VRM power status 1', 0xff, 0xff, ''),
    0x0d: ('DC VRM power status 2', 0xff, 0xfb, ''),
    0x0e: ('DC VRM power status 3', 0xff, 0xff, ''),
    0x1c: ('watchdog ticks low', 0, 0xff, '50 ms ticks, with 0x1d[3:0]'),
    0x1d: ('watchdog ticks high / ctrl', 0, 0x0f, 'b5:4 trigger sequence'),
    0x1f: ('clock gen MB status', 0, 0xc3, ''),
    0x20: ('system LED 1', 0, 0xff, '[2:0] sys LED, [4:3] PWR1, [6:5] PWR2; U-Boot writes 0x7d'),
    0x21: ('system LED 2', 0, 0xbf, '[1:0] fan LED, b7/b5/b4 LED test modes'),
    0x22: ('watchdog cause action', 0, 0x00, 'b1 watchdog enable'),
    0x23: ('over-temp action', 0x3f, 0x00, ''),
    0x24: ('voltage issue action', 0x3f, 0x00, ''),
    0x25: ('CPU reset action', 0, 0x00, 'reset-on-reboot select; install.sh writes 0x02'),
    0x26: ('software reset action', 0x3f, 0x00, 'WRITE = board reset; b0 port LED flash'),
    0x30: ('fault log status/ctrl', 0, 0xff, 'b7 pause, b5 reset, b1:0 read cmd/busy'),
    0x31: ('fault log addr 0', 0, 0xff, ''), 0x32: ('fault log addr 1', 0, 0xff, ''), 0x33: ('fault log addr 2', 0, 0xff, ''),
    0x34: ('fault log write data', 0, 0xff, ''),
    0x35: ('fault log read data', None, None, 'EXCLUDED: data port'),
    0x40: ('console log status/ctrl', 0, 0xff, 'b7 pause, b6 115200 (0 = 9600, v13 default), b5 reset; live 0x40'),
    0x41: ('console log addr 0', 0, 0xff, ''), 0x42: ('console log addr 1', 0, 0xff, ''), 0x43: ('console log addr 2', 0, 0xff, ''),
    0x44: ('console log write data', 0, 0xff, ''),
    0x45: ('console log read data', None, None, 'EXCLUDED: data port'),
    0x50: ('voltages OK 0', 0xff, 0xff, ''), 0x51: ('voltages OK 1', 0xff, 0xff, ''), 0x52: ('voltages OK 2', 0x0f, 0x0f, ''),
    0x53: ('under-voltage 0', 0xff, 0xff, ''), 0x54: ('under-voltage 1', 0xff, 0xff, ''), 0x55: ('under-voltage 2', 0x0f, 0x0f, ''),
    0x56: ('ASC0 temp interrupt', 0x3f, 0x00, 'latch candidate'), 0x57: ('ASC1 temp interrupt', 0x3f, 0x00, 'latch candidate'),
    0x58: ('timestamp b0', 0, 0, '32-bit 50 ms counter (U-Boot "Timestamp Now")'), 0x59: ('timestamp b1', 0, 0, ''),
    0x5a: ('timestamp b2', 0, 0, ''), 0x5b: ('timestamp b3', 0, 0, ''),
    0x74: ('voltages OK 3', 0x7f, 0x7f, ''), 0x75: ('voltages OK 4', 0xff, 0xff, ''), 0x76: ('voltages OK 5', 0x03, 0x03, ''),
    0x77: ('under-voltage 3', 0x7f, 0x7f, ''), 0x78: ('under-voltage 4', 0xff, 0xff, ''), 0x79: ('under-voltage 5', 0x03, 0x03, ''),
    0x7a: ('ASC2 temp interrupt', 0x3f, 0x00, 'latch candidate'), 0x7b: ('ASC3 temp interrupt', 0x3f, 0x00, 'latch candidate'),
    0x7c: ('ASC3 GPIO', 0, 0x07, ''),
    0x7e: ('JTAG chain select', 0, 0x00, '00 sys, 04/0c fan, 10 SWLED, 12 MDIO+MACLED, 20 CPU JTAG buffer off'),
    0xa0: ('clock gen MS status', 0, 0x03, ''),
    0xa1: ('I2C FPGA misc IO', 0xf0, 0xd3, 'b7 LM75 int, b5 present, b3 aux SPI flash to SoC, b2 EEPROM WP'),
    0xa3: ('fan 1 misc IO', 0, 0x0f, ''),
    0xa4: ('MACsec misc IO', 0, 0xff, 'b6 present, b5 LM75 int'),
    0xa5: ('MACsec DC VRM', 0x3f, 0x3f, 'b7 XP3R3V_PLL_PG added in v13'),
    0xa6: ('MACsec 0.9V', 0, 0x3f, ''),
    0xa7: ('I2C FPGA DC VRM', 0x0f, 0x0f, ''),
    0xa8: ('AC PSU 3/4 status', 0, 0xff, 'b5/4 present, b3/2 input ok, b1/0 output ok'),
    0xf8: ('margin ASC1 trim3 ctl', 0, 0, 'margin scripts only; never write'), 0xf9: ('margin ASC0/1 enable', 0, 0, ''),
    0xfa: ('margin ASC1 trim3 low', 0, 0, ''), 0xfb: ('margin ASC1 trim3 high', 0, 0, ''),
    0xfc: ('margin ASC2(/ASC3) enable', 0, 0, ''), 0xfd: ('margin ASC2/3 mask', 0, 0, ''),
    0xfe: ('margin ASC2(/3) low', 0, 0, ''), 0xff: ('margin ASC2(/3) high', 0, 0, ''),
}
LIVE = {0x00: 0x15, 0x09: 0x05, 0x20: 0x7d, 0x40: 0x40}   # vendor U-Boot prompt, 2026-11-15
LED = {
    0x00: ('version', 0, 0, 'v1 image = 0x01; SWLED runs 0x02, MACLED 0x01'),
    0x01: ('scratch', 0xff, 0xde, ''),
    0x09: ('port LED mode', 0x01, 0x00, 'b0 led_enable: 1 = CPU test colour from 0x0a'),
    0x0a: ('port LED test colour', 0, 0x03, '[2:0] 6 red, 3 green, 5 blue, 0 white, 7 off'),
}
SYS_EXCLUDE = {0x35, 0x45}
SYS_LATE = [0x07, 0x56, 0x57, 0x7a, 0x7b]
DEV = {  # name: (addr, mux selects, deselects, table, live version)
    'sys-cpld': (0x60, 'i2c mw 0x74 0 1; i2c mw 0x71 0 4', 'i2c mw 0x71 0 0; i2c mw 0x74 0 0', SYS),
    'swled-cpld': (0x64, 'i2c mw 0x74 0 1; i2c mw 0x71 0 0x20', 'i2c mw 0x71 0 0; i2c mw 0x74 0 0', LED),
    'macled-cpld': (0x63, 'i2c mw 0x74 0 2; i2c mw 0x73 0 1', 'i2c mw 0x73 0 0; i2c mw 0x74 0 0', LED),
}


def runs(regs):
    out = []
    for r in sorted(regs):
        if out and r == out[-1][1] + 1:
            out[-1][1] = r
        else:
            out.append([r, r])
    return out


def uboot():
    lines = ['i2c dev 3']
    a, sel, desel, _ = DEV['sys-cpld']
    bulk = [r for r in range(256) if r not in SYS_EXCLUDE and r not in SYS_LATE]
    lines.append('echo === sys-cpld ===; ' + sel)
    for s, e in runs(bulk):
        lines.append('i2c md 0x%02x 0x%02x 0x%x' % (a, s, e - s + 1))
    lines.append('echo === sys-cpld-late ===')
    for r in SYS_LATE:
        lines.append('i2c md 0x%02x 0x%02x 1' % (a, r))
    lines.append(desel)
    for name in ('swled-cpld', 'macled-cpld'):
        a, sel, desel, _ = DEV[name]
        lines.append('echo === %s ===; %s; i2c md 0x%02x 0x00 0x10; i2c md 0x%02x 0x10 0xf0; %s' % (name, sel, a, a, desel))
    print('\n'.join(lines))


def parse(path):
    data, cur = {}, None
    for line in open(path, errors='replace'):
        m = re.search(r'=== ([a-z-]+) ===', line)
        if m and 'echo' not in line:
            cur = m.group(1).replace('-late', '')
            data.setdefault(cur, {})
            continue
        m = re.match(r'\s*([0-9a-fA-F]{4}):((?: [0-9a-fA-F]{2})+)', line)
        if m and cur:
            base = int(m.group(1), 16)
            for i, b in enumerate(m.group(2).split()):
                data[cur][base + i] = int(b, 16)
    return data


def diff(path):
    data = parse(path)
    for name, (addr, _, _, table) in DEV.items():
        regs = data.get(name, {})
        print('== %s @0x%02x: %d registers captured' % (name, addr, len(regs)))
        for r in sorted(set(regs) | set(table)):
            v = regs.get(r)
            nm, mask, dflt, note = table.get(r, ('(undocumented)', 0, 0, ''))
            flag = ''
            if v is None:
                flag = 'not read' if mask is not None else 'excluded'
            elif mask and (v & mask) != (dflt & mask):
                flag = 'DIFF vs v13-era expected (mask %02x: want %02x got %02x)' % (mask, dflt & mask, v & mask)
            elif name == 'sys-cpld' and r in LIVE and v != LIVE[r]:
                flag = 'changed since 11-15 read (%02x)' % LIVE[r]
            elif r not in table and v not in (0x00, 0xff):
                flag = 'UNDOCUMENTED, non-trivial value'
            if flag or r in table:
                print('  %02x  %-4s %-36s %s %s' % (r, '%02x' % v if v is not None else '--', nm, flag, note))
        und = [r for r in regs if r not in table]
        vals = sorted({regs[r] for r in und})
        print('  undocumented registers read: %d, distinct values %s' % (len(und), ' '.join('%02x' % x for x in vals[:16])))


if __name__ == '__main__':
    if len(sys.argv) >= 2 and sys.argv[1] == 'uboot':
        uboot()
    elif len(sys.argv) == 3 and sys.argv[1] == 'diff':
        diff(sys.argv[2])
    else:
        print(__doc__)
        sys.exit(2)
