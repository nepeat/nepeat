# loop400_fwd.py: hardware forwarding for a front-port loop cable on the
# CS8210 (Teralynx 7, vendor SDK 0.1.7, knet mode), Python 2.
#
# Runs inside innovium.user (copy into the SDK's scripts/ dir in the chroot):
#   ./cls_shell 'run loop400_fwd add [A B]'    # default A=1 B=16 (devports)
#   ./cls_shell 'run loop400_fwd drop|fwd|del|show [A B]'
#
# With a cable between devports A and B, a frame sent out of A arrives on B.
# knet gives devport d its own L2VNI d+1 (members CPU + d, flood to CPU only),
# so normally it ends up on the CPU (fppB). This adds two static FDB entries
#   L2VNI(B+1): MAC_A -> sysport A    L2VNI(A+1): MAC_B -> sysport B
# (plus A in L2VNI(B+1) and B in L2VNI(A+1)), so frames with DMAC MAC_A
# injected on fppA circulate A -> cable -> B -> ASIC -> A ... at line rate
# (the egress queue holds what does not fit), MAC_B the other way. The ASIC
# is the traffic generator; the CPU only injects the first frames.
# 'drop' turns both entries into drop entries (stops the circulation, as the
# vendor snake test does), 'fwd' back, 'del' removes everything added.
# Nothing here floods: MAC_A/MAC_B are known unicast, other traffic keeps
# the knet behaviour. A restart of cs8210-switch removes it too.
import sys
from ctypes import c_uint8, c_uint32, pointer

I = sys.modules['ifcs_ctypes']
MAC_A = (0x02, 0x00, 0x00, 0x00, 0x40, 0x0a)  # leaves A, comes back on B
MAC_B = (0x02, 0x00, 0x00, 0x00, 0x40, 0x0b)  # leaves B, comes back on A


def ok(rc):
    return I.IFCS_STATUS_REASON(rc) == I.IFCS_SUCCESS


def sysport(dp):
    attr = I.ifcs_attr_t()
    n = c_uint32()
    attr.id = I.IFCS_DEVPORT_ATTR_SYSPORT
    rc = I.ifcs_devport_attr_get(0, dp, 1, pointer(attr), pointer(n))
    assert ok(rc), 'sysport of devport %d: rc %d' % (dp, rc)
    h = I.ifcs_handle_t()
    h.value = I.IFCS_HANDLE_SYSPORT(I.IFCS_HANDLE_VALUE(attr.value.handle))
    return h


def vni(v):
    h = I.ifcs_handle_t()
    h.value = I.IFCS_HANDLE_L2VNI(v)
    return h


def key(mac, v):
    k = I.ifcs_l2_entry_key_t()
    I.ifcs_l2_entry_key_t_init(pointer(k))
    I.ifcs_l2_entry_key_t_key_type_set(pointer(k), I.IFCS_L2_ENTRY_KEY_TYPE_MAC_L2VNI)
    I.ifcs_l2_entry_key_t_mac_l2vni_set(pointer(k), k.key.mac_l2vni)
    I.ifcs_l2_entry_key_mac_l2vni_t_mac_addr_set(pointer(k.key.mac_l2vni), (c_uint8 * 6)(*mac))
    I.ifcs_l2_entry_key_mac_l2vni_t_l2vni_set(pointer(k.key.mac_l2vni), vni(v))
    return k


def l2_add(mac, v, dp):
    attr = (I.ifcs_attr_t * 2)()
    attr[0].id = I.IFCS_L2_ENTRY_ATTR_ENTRY_DEST
    attr[0].value.handle = sysport(dp)
    attr[1].id = I.IFCS_L2_ENTRY_ATTR_ENTRY_TYPE
    attr[1].value.u32 = I.IFCS_L2_ENTRY_TYPE_STATIC
    rc = I.ifcs_l2_entry_create(0, pointer(key(mac, v)), 2, pointer(attr[0]))
    print('l2_entry_create vni %d -> devport %d: rc %d' % (v, dp, rc))


def l2_policy(mac, v, action):
    attr = (I.ifcs_attr_t * 1)()
    pol = I.ifcs_fwd_policy_t()
    I.ifcs_fwd_policy_t_init(pointer(pol))
    pol.fwd_action = action
    I.ifcs_attr_t_id_set(pointer(attr[0]), I.IFCS_L2_ENTRY_ATTR_ENTRY_FWD_POLICY)
    I.ifcs_attr_t_value_fwd_policy_set(pointer(attr[0]), pointer(pol))
    rc = I.ifcs_l2_entry_attr_set(0, pointer(key(mac, v)), 1, pointer(attr[0]))
    print('l2_entry fwd_policy vni %d action %d: rc %d' % (v, action, rc))


def member(v, dp, add):
    # Arrays as in the vendor knet.py (indexing a simple-type array gives a
    # Python int, so pointer(m[0]) does not work).
    m = (I.ifcs_handle_t * 1)()
    m[0] = sysport(dp).value
    if add:
        a = (I.ifcs_attr_t * 1)()
        rc = I.ifcs_l2vni_member_add(0, vni(v), 1, pointer(m), 0, pointer(a))
    else:
        rc = I.ifcs_l2vni_member_remove(0, vni(v), 1, pointer(m))
    print('l2vni %d member %s devport %d: rc %d' % (v, 'add' if add else 'remove', dp, rc))


def main(args=''):
    a = args.split()
    cmd = a[0] if a else 'show'
    dpa = int(a[1]) if len(a) > 1 else 1
    dpb = int(a[2]) if len(a) > 2 else 16
    va, vb = dpa + 1, dpb + 1  # knet: devport d -> L2VNI d+1
    if cmd == 'add':
        member(vb, dpa, True)
        member(va, dpb, True)
        l2_add(MAC_A, vb, dpa)
        l2_add(MAC_B, va, dpb)
    elif cmd == 'drop':
        l2_policy(MAC_A, vb, I.IFCS_FWD_ACTION_DROP)
        l2_policy(MAC_B, va, I.IFCS_FWD_ACTION_DROP)
    elif cmd == 'fwd':
        l2_policy(MAC_A, vb, I.IFCS_FWD_ACTION_FORWARD)
        l2_policy(MAC_B, va, I.IFCS_FWD_ACTION_FORWARD)
    elif cmd == 'del':
        for mac, v in ((MAC_A, vb), (MAC_B, va)):
            rc = I.ifcs_l2_entry_delete(0, pointer(key(mac, v)))
            print('l2_entry_delete vni %d: rc %d' % (v, rc))
        member(vb, dpa, False)
        member(va, dpb, False)
    elif cmd != 'show':
        print('usage: run loop400_fwd add|drop|fwd|del|show [A B]')
        return
    print('MAC_A %s in L2VNI %d -> devport %d; MAC_B %s in L2VNI %d -> devport %d' % (
        ':'.join('%02x' % b for b in MAC_A), vb, dpa,
        ':'.join('%02x' % b for b in MAC_B), va, dpb))
