#!/usr/bin/env python3
"""MACsec on the Evora PHYs: call the vendor library's bcm_plp_cfye_* / bcm_plp_secy_* API under the re-host.

The vendor 'extphy' shell has no key commands (only 'init ... mode=2' with a fixed test key). This tool calls the
library's MACsec dispatch table directly, with our parameters, in the same emulator as rehost.py. Addresses and
argument structures: ../macsec.md (structures = public epdm_bcm_common_sec_defines.h, checked against the binary).

  macsec.py [--backend mock|ssh|diag] [--load-state S] [--save-state S] [--trace F] CMD...
    CMD (one shell argument each, run in order):
      selftest                      build the vendor's test SA with the library and compare with its static record
      extphy <sub-command ...>      a vendor extphy command, as rehost.py does
      adopt <phy>                   take over the vPort / SA / rule handles that 'extphy init <phy> <n> mode=2' made
      on <phy> key=<hex> sci=<hex16> [an=0] [side=both|egress|ingress] [pn=1] [window=0] [co=0]
                                    [salt=<hex24> ssci=<hex8>] [protect=1] [encrypt=1] [validate=2] [replay=0]
                                    vPort + SA + match-all rule, as the vendor mode 2 does but with our key
      off <phy> [side=...]          remove rule, SA and vPort made by 'on' / 'adopt'
      sa-add / sa-del <phy> ...     only the SA (same options as 'on'); the vPort and rule stay
      bypass <phy> 0|1 [side=...]   bcm_plp_secy_bypass_set;   'bypass? <phy>' reads it back
      stats <phy> [side=...]        SA, SecY, interface and classifier counters
      mib <phy>                     non-zero port-MAC counters, line and system side (reg64 0x41000000..80)
      reg <phy> <addr>... [if_side=0|1] [reg64=1]   library register read (0x44.. / 0x46.. = MACsec engines)
      devinit <phy> macsec|bypass   uninit + init of the MACsec engines without a firmware download (untested on hardware)
      diag <diag-shell command>     ssh / diag backend: run a diag-shell command on the switch (ASIC side of a test)
      show                          the handle registry
  Handles are kept in <state>.macsec.json beside the --save-state / --load-state file.
  The mock backend models only the LMI register interface (lmi.py): it proves marshalling and call order, not the chip.
"""
import argparse, hashlib, json, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rehost, mdio, lmi

# ---- vendor binary (sha256 e163e815...): MACsec dispatch table, slot address -> see ../macsec.md -----------------
SLOT = {
    'cfye_device_init': 0x12dc3674, 'cfye_device_uninit': 0x12dc3678, 'cfye_vport_add': 0x12dc367c,
    'cfye_vport_remove': 0x12dc3680, 'cfye_vport_update': 0x12dc3684, 'cfye_rule_add': 0x12dc3688,
    'cfye_rule_remove': 0x12dc368c, 'cfye_rule_update': 0x12dc3690, 'cfye_rule_enable': 0x12dc3694,
    'cfye_rule_disable': 0x12dc3698, 'cfye_vport_id_get': 0x12dc36a0, 'cfye_device_limits': 0x12dc36a4,
    'cfye_device_update': 0x12dc36a8, 'cfye_statistics_read': 0x12dc36ac,
    'secy_device_init': 0x12dc36f8, 'secy_device_uninit': 0x12dc36fc, 'secy_sa_add': 0x12dc3700,
    'secy_sa_update': 0x12dc3704, 'secy_sa_read': 0x12dc3708, 'secy_sa_remove': 0x12dc370c,
    'secy_rules_sectag_update': 0x12dc3710, 'secy_bypass_get': 0x12dc3714, 'secy_bypass_set': 0x12dc3718,
    'secy_config_set': 0x12dc371c, 'secy_device_limits': 0x12dc3720, 'secy_sa_nextpn_update': 0x12dc3754,
    'secy_sa_chain': 0x12dc3758, 'secy_sa_switch': 0x12dc375c, 'secy_sa_statistics_e_get': 0x12dc3760,
    'secy_sa_statistics_i_get': 0x12dc3764, 'secy_ifc_statistics_e_get': 0x12dc3768,
    'secy_ifc_statistics_i_get': 0x12dc376c, 'secy_rxcam_statistics_get': 0x12dc3770,
    'secy_secy_statistics_e_get': 0x12dc3788, 'secy_secy_statistics_i_get': 0x12dc378c,
    'secy_device_update': 0x12dc37b0, 'build_transform_record': 0x12dc37b4,
}
# expected implementation behind each slot (catches a different binary or a wrong table)
IMPL = {'cfye_vport_add': 0x1181aa54, 'cfye_rule_add': 0x1181af58, 'secy_sa_add': 0x11811e2c,
        'secy_sa_remove': 0x118127c8, 'secy_bypass_set': 0x11812e44, 'build_transform_record': 0x118183b0,
        'secy_sa_statistics_e_get': 0x11815464, 'secy_secy_statistics_i_get': 0x11815974}
# vendor mode 2 ('extphy init ... mode=2', 0x11745990): static test SA and where it keeps its handles
VENDOR_TR = {0: 0x12d9debc, 1: 0x12d9de5c}          # egress / ingress transform record, 24 words each
VENDOR_SCI = 0x12331c8c
VENDOR_HANDLES = {'vport': 0x133ec738, 'vport_id': 0x133ed378, 'sa': 0x133eebb8, 'rule': 0x133edfa0}  # + phy*8 + side*4
DIAG_REG_GET, DIAG_REG64_GET = 0x1173a140, 0x1173a250   # diag wrappers (phy, lane, if_side, devad, reg, &val); lane 4 = all
DIAG_MACSEC_INIT = 0x1173aa80                            # (sec_access *, bypass): cfye_device_init + secy_device_init
# 0x0f / 0x53 are what the vendor's 'reg64 chkmiberr' wants zero; names = CLMAC MIB order as in phystat.py (inferred)
MIB_NAMES = {0x0b: 'RPKT', 0x0f: 'RFCS', 0x18: 'RERPKT', 0x1a: 'ROVR', 0x34: 'RUND', 0x3d: 'RBYT', 0x4c: 'TPKT',
             0x53: 'TFCS', 0x5d: 'TERR', 0x6f: 'TBYT'}
SA_WORDS = 24
EGRESS, INGRESS = 0, 1
ACTION_INGRESS, ACTION_EGRESS = 2, 3                 # bcm_plp_secy_sa_action_type_t
DROP_INTERNAL, PORT_CONTROLLED = 2, 2

SA_STAT_E = ['out_pkts_encrypted_protected', 'out_pkts_too_long', 'out_pkts_sa_not_inuse', 'out_octets_encrypted_protected']
SA_STAT_I = ['in_pkts_unchecked', 'in_pkts_delayed', 'in_pkts_late', 'in_pkts_ok', 'in_pkts_invalid', 'in_pkts_not_valid',
             'in_pkts_not_using_sa', 'in_pkts_unused_sa', 'in_octets_decrypted', 'in_octets_validated']
SECY_STAT_E = ['out_pkts_transform_error', 'out_pkts_control', 'out_pkts_untagged']
SECY_STAT_I = ['in_pkts_transform_error', 'in_pkts_control', 'in_pkts_untagged', 'in_pkts_no_tag', 'in_pkts_badtag',
               'in_pkts_no_sci', 'in_pkts_unknown_sci', 'in_pkts_tagged_ctrl']
IFC_STAT_E = ['out_pkts_unicast_uncontrolled', 'out_pkts_multicast_uncontrolled', 'out_pkts_broadcast_uncontrolled',
              'out_rx_drop_pkts_uncontrolled', 'out_rx_err_pkts_uncontrolled', 'out_pkts_unicast_controlled',
              'out_pkts_multicast_controlled', 'out_pkts_broadcast_controlled', 'out_rx_drop_pkts_controlled',
              'out_rx_err_pkts_controlled', 'out_octets_uncontrolled', 'out_octets_controlled', 'out_octets_common']
IFC_STAT_I = ['in_pkts_unicast_uncontrolled', 'in_pkts_multicast_uncontrolled', 'in_pkts_broadcast_uncontrolled',
              'in_rx_drop_pkts_uncontrolled', 'in_rx_err_pkts_uncontrolled', 'in_pkts_unicast_controlled',
              'in_pkts_multicast_controlled', 'in_pkts_broadcast_controlled', 'in_rx_drop_pkts_controlled',
              'in_rx_err_pkts_controlled', 'in_octets_uncontrolled', 'in_octets_controlled']


class ApiError(Exception):
    pass


def aes_ecb(key, block):
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    e = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
    return e.update(block) + e.finalize()


class ChipMock(mdio.Mock):
    """mdio.Mock plus the LMI register model, so library calls that go through LMI complete."""

    def __init__(self, preset=None):
        super().__init__()
        self.lmi = lmi.Model(preset)
        self.events = self.lmi.log          # decoded LMI operations and firmware messages, in order
        self.msg_open, self.ack = set(), {}

    def read(self, phy, devad, reg):
        if self.lmi.handles(devad, reg):
            return self.lmi.read(phy, reg)
        return super().read(phy, devad, reg)

    def write(self, phy, devad, reg, val):
        if self.lmi.handles(devad, reg):
            return self.lmi.write(phy, reg, val & 0xffff)
        super().write(phy, devad, reg, val)
        if devad == 1 and reg == 0xa000:
            self.msg_open.add(phy)
            self.ack.pop(phy, None)
        if devad == 1 and reg == 0x8222 and val & 0xff00 in (0x1100, 0x1200) and phy in self.msg_open:
            # firmware message (data in 1.a000..): the running firmware answers 0x11e3 / 0x12e3 in 1.8221
            self.msg_open.discard(phy)
            super().write(phy, 1, 0x8221, (val & 0xff00) | 0xe3)
            self.ack[phy] = (val & 0xff00) | 0xe3
            self.events.append('msg %03x %04x %s' % (phy, val, self.message(phy).hex()))

    def message(self, phy):
        """Bytes of the firmware message in 1.a000.. (16-bit words, low byte first; byte 0 = bytes after the first two)."""
        n = (self.regs.get((phy, 1, 0xa000), 0) & 0xff) + 2
        b = bytearray()
        for i in range((n + 1) // 2):
            w = self.regs.get((phy, 1, 0xa000 + i), 0)
            b += bytes((w & 0xff, w >> 8))
        return bytes(b[:n])


class OracleMock(ChipMock):
    """ChipMock whose direct (non-LMI) registers answer with what a real PHY answered in a recorded trace: the n-th
    read of a register returns the n-th recorded value of that register (then the last one). Good enough to walk
    the vendor init offline; it is a recording, not a chip model."""

    def __init__(self, path, src_phy, preset=None):
        super().__init__(preset)
        self.seq, self.pos = {}, {}
        lseq, lpos = {}, {}
        for op in lmi.decode(open(path)):
            if op.phy == src_phy and not op.write:
                n = op.width // 2
                for i in range(op.count):
                    lseq.setdefault(op.key()[1:4] + (op.full_addr + i,), []).append(op.words[i * n:(i + 1) * n])
        model_value = self.lmi.value

        def value(k, n):                      # LMI reads: the n-th recorded read of that register, if any
            s = lseq.get(k[1:])
            if s is None:
                return model_value(k, n)
            i = lpos.get(k, 0)
            lpos[k] = i + 1
            return (list(s[min(i, len(s) - 1)]) + [0] * n)[:n]
        self.lmi.value = value
        for l in open(path):
            f = l.split()
            if len(f) == 5 and f[0] == 'R' and int(f[1], 16) == src_phy and not self.lmi.handles(int(f[2], 16), int(f[3], 16)):
                self.seq.setdefault((int(f[2], 16), int(f[3], 16)), []).append(int(f[4], 16))

    def read(self, phy, devad, reg):
        s = self.seq.get((devad, reg))
        if s is None or self.lmi.handles(devad, reg) or (devad == 1 and reg == 0x8221 and phy in self.ack):
            return super().read(phy, devad, reg)
        i = self.pos.get((phy, devad, reg), 0)
        self.pos[(phy, devad, reg)] = i + 1
        return s[min(i, len(s) - 1)]


def learn_preset(path, phy=None):
    """Register values the real chip returned where they differ from what was last written (IDs, status)."""
    last, ro = {}, {}
    for op in lmi.decode(open(path)):
        if phy is not None and op.phy != phy:
            continue
        n = op.width // 2
        for i in range(op.count):
            k = ('any',) + op.key()[1:4] + (op.full_addr + i,)
            w = op.words[i * n:(i + 1) * n]
            if op.write:
                last[k] = w
            elif last.get(k) != w:
                ro[k] = w
    return ro


class Macsec:
    def __init__(self, emu, load_path=None, save_path=None):
        """load_path / save_path: the handle registry beside the library state it belongs to."""
        self.e = emu
        self.path = save_path
        self.reg = {}
        if load_path and os.path.exists(load_path):
            self.reg = json.load(open(load_path))
        # buffers handed to the library live in the emulator's scratch area: keep them in saved states
        base, regions = emu.scratch, emu.regions
        emu.regions = lambda: regions() + [(base, max(emu.scratch - base, 0x1000))]
        for name, impl in IMPL.items():
            got = emu.u32(SLOT[name])
            if got != impl:
                raise ApiError('dispatch slot %s holds %08x, expected %08x: wrong binary?' % (name, got, impl))

    # ---- marshalling ----------------------------------------------------------------------------------
    def buf(self, data):
        return self.e.alloc(bytes(data))

    def access(self, phy, side, lane_map=0xf):
        """bcm_plp_sec_phy_access_t, 0x3c bytes in this binary: {platform_ctxt, phy_addr, if_side, lane_map, flags, +1 word}
        (one word more than the public header), 8 mutex hooks (the library fills them), macsec_side at +0x38."""
        return self.buf(struct.pack('>6I', 5, phy, 0, lane_map, 0, 0) + b'\0' * 32 + struct.pack('>I', side))

    def call(self, name, *args):
        fn = self.e.u32(SLOT[name])
        if not fn:
            raise ApiError('%s: not implemented by the vendor library' % name)
        r = self.e.call(fn, *args)
        if hasattr(self.e.be, 'flush'):
            self.e.be.flush()
        return r - (1 << 32) if r & 0x80000000 else r

    def check(self, name, *args):
        r = self.call(name, *args)
        if r != 0:
            raise ApiError('%s returned %d%s' % (name, r, ' (PHY not initialised in this library state)' if r == -25 else ''))
        return r

    def slot(self, phy, side):
        return self.reg.setdefault('%#x' % phy, {}).setdefault(str(side), {})

    def save(self):
        if self.path:
            with open(self.path, 'w') as f:
                json.dump(self.reg, f, indent=1, sort_keys=True)

    # ---- transform record ---------------------------------------------------------------------------------
    def build_record(self, side, key, sci, an=0, pn=1, window=0, salt=None, ssci=None, pn_hi=0, flags=0):
        """bcm_plp_secy_build_transform_record: 24 words. H = AES_K(0) is the caller's job; salt+ssci select XPN."""
        if len(key) not in (16, 32) or len(sci) != 8:
            raise ApiError('key must be 16 or 32 bytes, SCI 8 bytes')
        if (salt is None) != (ssci is None) or (salt is not None and (len(salt) != 12 or len(ssci) != 4)):
            raise ApiError('XPN needs salt (12 bytes) and ssci (4 bytes)')
        h = aes_ecb(key, b'\0' * 16)
        p = struct.pack('>3IB3x10I', flags, side, 0, an, self.buf(key), len(key), self.buf(h),
                        self.buf(salt) if salt else 0, self.buf(ssci) if ssci else 0, self.buf(sci),
                        pn, pn_hi, window, 0)
        out = self.buf(b'\0' * (SA_WORDS * 4))
        self.check('build_transform_record', self.access(0, side), self.buf(p), out)
        return bytes(self.e.uc.mem_read(out, SA_WORDS * 4))

    def selftest(self):
        """The library must rebuild the vendor's static mode-2 SA from the key and SCI that record contains."""
        ok = True
        sci = bytes(self.e.uc.mem_read(VENDOR_SCI, 8))
        for side in (EGRESS, INGRESS):
            want = bytes(self.e.uc.mem_read(VENDOR_TR[side], SA_WORDS * 4))
            w = struct.unpack('>24I', want)
            key = struct.pack('<4I', *w[2:6])                      # record words are byte-swapped key words
            pn, window = w[14], w[16]
            got = self.build_record(side, key, sci, an=0, pn=pn, window=window, flags=8)   # 8 = allow PN roll-over
            g = struct.unpack('>24I', got)
            diff = [i for i in range(SA_WORDS) if g[i] != w[i] and i != 1]                 # word 1 = free context id
            hk = struct.pack('<4I', *w[10:14])
            print('selftest %s: key %s SCI %s pn %d window %#x  H %s  -> %s' % (
                'ingress' if side else 'egress ', hashlib.sha256(key).hexdigest()[:12] + '(sha256)', sci.hex(), pn, window,
                'ok' if hk == aes_ecb(key, b'\0' * 16) else 'MISMATCH',
                'identical to the vendor record (flags 8, context id %#x aside)' % w[1] if not diff else 'differs in words %s' % diff))
            if diff:
                ok = False
                for i in diff:
                    print('   word %2d: built %08x, vendor %08x' % (i, g[i], w[i]))
        return ok

    # ---- objects ------------------------------------------------------------------------------------------
    def vport_add(self, phy, side):
        s = self.slot(phy, side)
        if 'vport' in s:
            return s
        a = self.access(phy, side)
        hp = self.buf(b'\0' * 4)
        self.check('cfye_vport_add', a, hp, self.buf(bytes([0 if side == EGRESS else 3]) + b'\0' * 7))  # pkt_extension as the vendor
        s['vport'] = self.e.u32(hp)
        s['vport_id'] = self.call('cfye_vport_id_get', a, s['vport'])
        # device exceptions: drop_action = DROP_INTERNAL, low-latency bypass off (the vendor does this per side)
        exc = self.buf(struct.pack('>IBBH', DROP_INTERNAL, 0, 0, 0))
        ctl = self.buf(struct.pack('>B3xI', 0, exc))
        self.check('cfye_device_update', a, self.buf(struct.pack('>4I', ctl, 0, 0, 0)))
        return s

    def sa_struct(self, side, rec, sci=None, an=0, co=0, protect=1, encrypt=1, validate=2, replay=0, include_sci=1):
        tr = self.buf(rec)
        if side == EGRESS:      # bcm_plp_secy_sa_e_t: sa_inuse, conf_offset, protect_frames, include_sci, use_es, use_scb, conf_protect, allow_data_pkts
            params = struct.pack('>8B', 1, co, protect, include_sci, 0, 0, encrypt, 1) + b'\0' * 12
            action = ACTION_EGRESS
        else:                   # bcm_plp_secy_sa_i_t
            params = struct.pack('>3BxIIB3B2B2x', 1, co, replay, validate, self.buf(sci), an, 1, 0, 0, 0, 0)
            action = ACTION_INGRESS
        assert len(params) == 20
        return self.buf(struct.pack('>II', SA_WORDS, tr) + params + struct.pack('>3I', action, DROP_INTERNAL, PORT_CONTROLLED))

    def sa_add(self, phy, side, key, sci, an=0, pn=1, window=0, salt=None, ssci=None, flags=0, **kw):
        s = self.slot(phy, side)
        if 'vport_id' not in s:
            raise ApiError('no vPort for phy %#x side %d: run on/adopt first' % (phy, side))
        rec = self.build_record(side, key, sci, an, pn, window, salt, ssci, flags=flags)
        hp = self.buf(b'\0' * 4)
        self.check('secy_sa_add', self.access(phy, side), s['vport_id'], hp, self.sa_struct(side, rec, sci, an, **kw))
        s.setdefault('sa', {})[str(an)] = self.e.u32(hp)
        return s

    def sa_del(self, phy, side, an=None):
        s = self.slot(phy, side)
        for k in list(s.get('sa', {})):
            if an is None or k == str(an):
                self.check('secy_sa_remove', self.access(phy, side), self.buf(struct.pack('>I', s['sa'][k])))
                del s['sa'][k]

    def rule_add(self, phy, side):
        s = self.slot(phy, side)
        if 'rule' in s:
            return s
        a = self.access(phy, side)
        # bcm_plp_cfye_rule_t as the vendor fills it: packet-type mask 3, data mask 0 = every packet -> this vPort
        r = struct.pack('>2I4I4IIBBBx', 0, 3, 0x09000000, 0x101, 0, 0, 0, 0, 0, 0, s['vport'], 0, 0, 0)
        hp = self.buf(b'\0' * 4)
        self.check('cfye_rule_add', a, s['vport'], hp, self.buf(r))
        s['rule'] = self.e.u32(hp)
        self.check('cfye_rule_enable', a, s['rule'], 1)
        return s

    def on(self, phy, side, key, sci, **kw):
        self.vport_add(phy, side)
        self.sa_add(phy, side, key, sci, **kw)
        self.rule_add(phy, side)

    def off(self, phy, side):
        s = self.slot(phy, side)
        a = self.access(phy, side)
        if 'rule' in s:
            self.check('cfye_rule_disable', a, s['rule'], 1)
            self.check('cfye_rule_remove', a, s['rule'])
            del s['rule']
        self.sa_del(phy, side)
        if 'vport' in s:
            self.check('cfye_vport_remove', a, s['vport'])
            del s['vport'], s['vport_id']

    def adopt(self, phy):
        """Handles of the objects the vendor's mode 2 created (its global arrays, index phy*8 + side*4)."""
        for side in (EGRESS, INGRESS):
            v = {k: self.e.u32(a + phy * 8 + side * 4) for k, a in VENDOR_HANDLES.items()}
            if not v['vport'] or not v['sa']:
                raise ApiError('phy %#x side %d: the vendor arrays hold no handle (was mode 2 run in this state?)' % (phy, side))
            s = self.slot(phy, side)
            s.update({'vport': v['vport'], 'vport_id': v['vport_id'], 'rule': v['rule'], 'sa': {'0': v['sa']}})

    def devinit(self, phy, bypass):
        """Re-initialise the MACsec engines of a PHY the way the vendor's init does (0x1173aa80, then
        secy_config_set and, for bypass, secy_bypass_set(1)); mode 2 = bypass 0. Unverified on hardware."""
        for side in (EGRESS, INGRESS):
            a = self.access(phy, side)
            for name in ('cfye_device_uninit', 'secy_device_uninit'):
                r = self.call(name, a)
                print('   %s side %d: %d' % (name, side, r))
            self.reg.get('%#x' % phy, {}).pop(str(side), None)
        for side in (EGRESS, INGRESS):
            r = self.e.call(DIAG_MACSEC_INIT, self.access(phy, side), 1 if bypass else 0)
            if r:
                raise ApiError('MACsec device init of phy %#x side %d returned %#x' % (phy, side, r))
        for side in (EGRESS, INGRESS):
            self.check('secy_config_set', self.access(phy, side))
        if bypass:
            for side in (EGRESS, INGRESS):
                self.bypass_set(phy, side, 1)

    def reg_get(self, phy, addr, if_side=0, wide=False):
        p = self.buf(b'\0' * 8)
        r = self.e.call(DIAG_REG64_GET if wide else DIAG_REG_GET, phy, 4, if_side, 1, addr, p)
        if r:
            raise ApiError('register read %#x returned %#x' % (addr, r))
        return struct.unpack('>Q' if wide else '>I', bytes(self.e.uc.mem_read(p, 8 if wide else 4)))[0]

    def mib(self, phy, if_side):
        """Non-zero counters of the PHY's port MAC (reg64 0x41000000 + n, what 'reg64 clearmibc' clears)."""
        return {n: v for n in range(0x81) for v in [self.reg_get(phy, 0x41000000 + n, if_side, True)] if v}

    def bypass_set(self, phy, side, on):
        self.check('secy_bypass_set', self.access(phy, side), 1 if on else 0)

    def bypass_get(self, phy, side):
        p = self.buf(b'\xa5' * 4)
        self.check('secy_bypass_get', self.access(phy, side), p)
        return self.e.uc.mem_read(p, 1)[0]

    # ---- counters -----------------------------------------------------------------------------------------
    def counters(self, name, names, phy, side, arg, by_ref=False):
        n = len(names) * 8
        p = self.buf(b'\xa5' * (n + 64))
        a2 = self.buf(struct.pack('>I', arg)) if by_ref else arg
        r = self.call(name, self.access(phy, side), a2, p, 1)
        if r != 0:
            return {'error': r}
        raw = bytes(self.e.uc.mem_read(p, n + 64))
        out = {nm: (struct.unpack('>II', raw[8 * i:8 * i + 8])[1] << 32) | struct.unpack('>II', raw[8 * i:8 * i + 8])[0]
               for i, nm in enumerate(names)}
        extra = len(raw[n:].rstrip(b'\xa5'))
        if extra:   # the vendor structure is longer than the public header's
            out['_unnamed_bytes'] = raw[n:n + extra].hex()
        return out

    def stats(self, phy, side):
        s = self.slot(phy, side)
        out = {}
        e = side == EGRESS
        for an, h in sorted(s.get('sa', {}).items()):
            out['sa an=%s' % an] = self.counters('secy_sa_statistics_e_get' if e else 'secy_sa_statistics_i_get',
                                                 SA_STAT_E if e else SA_STAT_I, phy, side, h, by_ref=True)
        if 'vport_id' in s:
            out['secy'] = self.counters('secy_secy_statistics_e_get' if e else 'secy_secy_statistics_i_get',
                                        SECY_STAT_E if e else SECY_STAT_I, phy, side, s['vport_id'])
            out['ifc'] = self.counters('secy_ifc_statistics_e_get' if e else 'secy_ifc_statistics_i_get',
                                       IFC_STAT_E if e else IFC_STAT_I, phy, side, s['vport_id'])
            out['classifier rule hits'] = self.counters('cfye_statistics_read', ['hit'], phy, side, 0)
            if not e:
                out['rxcam'] = self.counters('secy_rxcam_statistics_get', ['cam_hit'], phy, side, 0)
        return out


# ---- command line ---------------------------------------------------------------------------------------------
def sides(opt):
    return {'both': (EGRESS, INGRESS), 'egress': (EGRESS,), 'ingress': (INGRESS,)}[opt.get('side', 'both')]


def parse(words):
    pos, opt = [], {}
    for w in words:
        if '=' in w:
            k, v = w.split('=', 1)
            opt[k] = v
        else:
            pos.append(w)
    return pos, opt


def sa_args(opt):
    kw = dict(an=int(opt.get('an', '0'), 0), pn=int(opt.get('pn', '1'), 0), window=int(opt.get('window', '0'), 0),
              flags=int(opt.get('flags', '0'), 0))
    if 'salt' in opt or 'ssci' in opt:
        kw.update(salt=bytes.fromhex(opt['salt']), ssci=bytes.fromhex(opt['ssci']))
    return bytes.fromhex(opt['key']), bytes.fromhex(opt['sci']), kw


def flow_args(side, opt):
    kw = dict(co=int(opt.get('co', '0'), 0))
    if side == EGRESS:
        kw.update(protect=int(opt.get('protect', '1')), encrypt=int(opt.get('encrypt', '1')))
    else:
        kw.update(validate=int(opt.get('validate', '2')), replay=int(opt.get('replay', '0')))
    return kw


def run(m, e, line):
    w = line.split()
    cmd, (pos, opt) = w[0], parse(w[1:])
    if cmd == 'extphy':
        return e.extphy(' '.join(w[1:]))
    shown = ' '.join(x if not x.startswith(('key=', 'salt=')) else x.split('=')[0] + '=<%d bytes>' % (len(x.split('=')[1]) // 2)
                     for x in w)
    print('>>> ' + shown)
    if cmd in ('selftest', 'show'):
        return run2(m, e, cmd, pos, opt)
    if cmd == 'diag':
        return run2(m, e, cmd, pos, opt, line)
    # trace section markers in the form mkscript.py compiles ("# <command>" ... "# rc=0"); never the key
    e.trace.note('macsec ' + shown)
    try:
        run2(m, e, cmd, pos, opt)
    except Exception:
        e.trace.note('rc=1')
        raise
    e.trace.note('rc=0')
    m.save()


def run2(m, e, cmd, pos, opt, raw=''):
    if cmd == 'selftest':
        if not m.selftest():
            raise ApiError('selftest failed')
        return
    if cmd == 'show':
        print(json.dumps(m.reg, indent=1, sort_keys=True))
        return
    if cmd == 'diag':                      # a diag-shell command of our bcm.user, through the MDIO bridge
        line = raw.split(None, 1)[1]
        if not hasattr(e.be, 'cmd'):
            print('(mock backend: no diag shell)')
            return
        e.be.flush()
        print(e.be.cmd(line, 300).rstrip())
        return
    phy = int(pos[0], 0)
    if cmd == 'devinit':
        m.devinit(phy, {'macsec': 0, 'bypass': 1}[pos[1]])
    elif cmd == 'reg':
        for a in pos[1:]:
            print('phy %#x if_side %s %s = %#x' % (phy, opt.get('if_side', '0'), a,
                                                   m.reg_get(phy, int(a, 0), int(opt.get('if_side', '0')), 'reg64' in opt)))
    elif cmd == 'mib':
        for if_side in (0, 1):
            d = m.mib(phy, if_side)
            print('phy %#x %s MAC: %s' % (phy, 'system' if if_side else 'line  ',
                                          ' '.join('%s=%d' % (MIB_NAMES.get(n, '%02x' % n), v) for n, v in sorted(d.items())) or 'all zero'))
    elif cmd == 'adopt':
        m.adopt(phy)
    elif cmd in ('on', 'sa-add'):
        key, sci, kw = sa_args(opt)
        for s in sides(opt):
            kw2 = dict(kw, **flow_args(s, opt))
            (m.on if cmd == 'on' else m.sa_add)(phy, s, key, sci, **kw2)
    elif cmd == 'sa-del':
        for s in sides(opt):
            m.sa_del(phy, s, int(opt['an'], 0) if 'an' in opt else None)
    elif cmd == 'off':
        for s in sides(opt):
            m.off(phy, s)
    elif cmd == 'bypass':
        for s in sides(opt):
            m.bypass_set(phy, s, int(pos[1], 0))
    elif cmd == 'bypass?':
        for s in sides(opt):
            print('phy %#x %s: bypass %d' % (phy, 'ingress' if s else 'egress', m.bypass_get(phy, s)))
    elif cmd == 'stats':
        for s in sides(opt):
            for grp, d in m.stats(phy, s).items():
                print('phy %#x %s %s: %s' % (phy, 'ingress' if s else 'egress ', grp,
                                             ' '.join('%s=%s' % kv for kv in d.items())))
    else:
        raise ApiError('unknown command ' + cmd)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('cmds', nargs='+')
    ap.add_argument('--bin', default=rehost.DEFAULT_BIN)
    ap.add_argument('--backend', default='mock', choices=['mock', 'ssh', 'diag'],
                    help='ssh: private bcm.user (SONiC stopped); diag: through syncd (SONiC running), see mdio.py')
    ap.add_argument('--ssh', default='sudo ssh -T root@10.60.2.23')
    ap.add_argument('--bridge-log')
    ap.add_argument('--trace')
    ap.add_argument('--log')
    ap.add_argument('--load-state')
    ap.add_argument('--save-state')
    ap.add_argument('--learn', help='mock only: MDIO trace of a real PHY to take read-only register values from')
    ap.add_argument('--oracle', help='mock only: TRACE:PHYID, answer direct-register reads from that recorded PHY')
    ap.add_argument('--verbose', '-v', action='store_true')
    ap.add_argument('--lmi', action='store_true', help='mock only: print the decoded LMI register operations')
    a = ap.parse_args()
    a.itrace = a.strace = None
    h = hashlib.sha256(open(a.bin, 'rb').read()).hexdigest()
    if not h.startswith(rehost.BIN_SHA256_PREFIX):
        raise SystemExit('unexpected vendor binary (sha256 %s)' % h)
    e = rehost.Emu(a)
    if a.backend == 'mock':
        preset = learn_preset(a.learn) if a.learn else None
        if a.oracle:
            path, src = a.oracle.rsplit(':', 1)
            e.be = OracleMock(path, int(src, 0), preset)
        else:
            e.be = ChipMock(preset)
    rc = 0
    try:
        e.boot()
        if a.load_state:
            e.load_state(a.load_state)
        m = Macsec(e, a.load_state + '.macsec.json' if a.load_state else None,
                   a.save_state + '.macsec.json' if a.save_state else None)
        if a.backend != 'mock' and not a.save_state:
            sys.stderr.write('warning: no --save-state: handles and library state of this run are lost at exit\n')
        for c in a.cmds:
            try:
                run(m, e, c)
            finally:
                if a.lmi and a.backend == 'mock':
                    for x in e.be.events:
                        print('   ' + str(x))
                    del e.be.events[:]
        if a.save_state:
            e.save_state(a.save_state)
            m.save()
    except (ApiError, rehost.Stop, Exception) as ex:
        sys.stderr.write('STOPPED: %r\n' % (ex,))
        rc = 1
    finally:
        try:
            e.be.close()
        finally:
            e.trace.close()
    sys.stderr.write('trace: %s\n' % (e.trace.n,))
    return rc


if __name__ == '__main__':
    sys.exit(main())
