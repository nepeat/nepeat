# ACL + CoPP policers on the AS7760: switch test (2026-11-15)

What [sai-openbcm's field processor code](../../nix/packages/sai-openbcm/README.md#acl-and-copp-field-processor)
should do on the chip ([VM](sonic-acl-fake-vm-20261115.txt)). Run on the switch 2026-11-15:
[result](sonic-as7760-acl-copp-20261115.txt) (ARP over its cap: 818/s, open). Setup: the lab loop and VRFs of
[vrf-ecmp-test.md](vrf-ecmp-test.md) (Ethernet1/1 = Ethernet0 in the default VRF,
10.0.0.0/31; Ethernet32/1 = Ethernet124 in Vrf-test, 10.0.0.1/31; iBGP between
them up), the new system deployed (syncd restarts with it), and
[copp_flood.py](copp_flood.py) in /root. `D=openbcm-diag`,
`A="redis-cli -s /var/run/redis/redis.sock -n 1"`, `C="redis-cli -s /var/run/redis/redis.sock -n 4"`.

## 0. after boot: policers and CoPP entries exist

```sh
journalctl -b -u sonic-orchagent | grep -ci 'policer.*fail'          # 0 (was 6 NOT_IMPLEMENTED per boot)
$A --scan --pattern '*SAI_OBJECT_TYPE_POLICER*' | wc -l               # one per COPP_GROUP CoppOrch created (VM: see evidence)
journalctl -b -u sonic-syncd | grep -E 'CoPP: [0-9]+ field entries|field group|policer [0-9]+:' | tail -20
#   "policer N: sr_tcm packets cir 600 cbs 600 ...", "field group G: priority 1000, qualifiers 0x...",
#   last "CoPP: N field entries, P policers (field group G)": VM (no BGP/LLDP traps) 9 / 3;
#   with bgp + lldp trapped expect about 14 / 4 (ARP 2 + ND 5 + LACP 1 + BGP 4 + LLDP 1 + IP2ME 1)
journalctl -b -u sonic-syncd | grep -E 'CoPP: field (group|entry)|routed-to-CPU|refused to share'   # expect nothing
$D 'fp show group'                                                   # the CoPP group (priority 1000), its slices
```

If the CoPP group has no `DstL3Egress` (warning "without the routed-to-CPU
qualifier"), IP2ME/BGP are unpoliced: report the SDK error from the log.

## 1. ACL: drop ICMP to one address on Ethernet1/1

```sh
ip vrf exec Vrf-test ping -c 20 -i 0.05 10.0.0.0 | tail -2          # baseline 0% loss
$C hset 'ACL_TABLE|HWDROP' type L3 stage ingress policy_desc hwtest ports@ Ethernet0
$C hset 'ACL_RULE|HWDROP|ICMP' PRIORITY 9000 PACKET_ACTION DROP DST_IP 10.0.0.0/32 IP_PROTOCOL 1
sleep 3
$A --scan --pattern '*SAI_OBJECT_TYPE_ACL_ENTRY*' | wc -l          # 1
journalctl -b -u sonic-syncd | grep 'ACL entry: SDK entry' | tail -1   # "... 1 ports, drop, counted"
ip vrf exec Vrf-test ping -c 20 -i 0.05 10.0.0.0 | tail -2          # 100% packet loss
ping -c 20 -i 0.05 10.0.0.1 | tail -2                               # 100% too (the replies enter at Ethernet0)
counterpoll acl enable                                               # once: ACL_STAT_COUNTER polling is off by default
K=$($A hget "$($A --scan --pattern '*SAI_OBJECT_TYPE_ACL_ENTRY*')" SAI_ACL_ENTRY_ATTR_ACTION_COUNTER)
sleep 12; redis-cli -s /var/run/redis/redis.sock -n 2 hgetall "COUNTERS:$K"   # SAI_ACL_COUNTER_ATTR_PACKETS >= 40
#   (aclshow / show acl are not usable yet: aclshow is not installed, acl-loader lacks pyangbind)
vtysh -c 'show ip bgp vrf all summary' | grep 10.0.0                 # both sessions still Established (TCP not matched)
$D 'show c xe0' | grep -E 'RFILDR|RDISC'                             # FP drop counter grows with the pings
$C del 'ACL_RULE|HWDROP|ICMP'; sleep 3
ip vrf exec Vrf-test ping -c 20 -i 0.05 10.0.0.0 | tail -2          # 0% loss again
$C del 'ACL_TABLE|HWDROP'; sleep 3
$A --scan --pattern '*SAI_OBJECT_TYPE_ACL_*' | wc -l                 # 0
```

Failure modes to tell apart: no drop and the counter stays 0 = the
entry does not match (in-ports semantics, qualifier width; `$D 'fp show'`
shows the entry and its InPorts); count grows but pings pass = `Drop` does
not stop CPU-bound (IP2ME) packets: repeat with a routed destination
(vrf-ecmp-test step 3a, rule DST_IP 192.0.2.1/32 on Ethernet0: `ttl63 in: 0`).

## 2. CoPP: the policer caps what reaches the CPU

```sh
# IP2ME (queue1_group1, 6000 pps): ICMP to Vrf-test's own address, entering at Ethernet124
python3 copp_flood.py --dev Ethernet0 --rx Ethernet124 --kind icmp --src 10.0.0.0 --dst 10.0.0.1 --seconds 10
#   expect "rx +N" <= ~6000/s * 10 + 6000 burst while "sent" is higher; if the sender
#   is slower than 6000/s nothing is cut: lower the rate first:
$C hset 'COPP_GROUP|queue1_group1' cir 100 cbs 100; sleep 3           # coppmgrd -> CoppOrch -> policer set
journalctl -b -u sonic-syncd | grep -i 'policer' | tail -3             # no error
python3 copp_flood.py --dev Ethernet0 --rx Ethernet124 --kind icmp --src 10.0.0.0 --dst 10.0.0.1 --seconds 10
#   expect rx about 100/s (+100 burst): ~1100
$D 'show c cpu0' | head                                              # CPU queue counters: the same order
$C hset 'COPP_GROUP|queue1_group1' cir 6000 cbs 6000                  # back to the copp_cfg.json values
# ARP (queue4_group2, 600 pps, copy: only the CPU copy is cut)
python3 copp_flood.py --dev Ethernet0 --rx Ethernet124 --kind arp --src 10.0.0.0 --dst 10.0.0.1 --seconds 10
#   expect rx about 600/s (+600): ~6600 when the sender does more than that
```

Expected "wrong" results and what they mean: rx = sent (no cap) with the
CoPP entries present = the meter is not applied (`$D 'fp show'`: policer
attached? packet mode?); rx about 1000x off = packet-mode units (pps taken as
kbit/s); ARP capped but forwarding broken = `RpCopyToCpuCancel` acting as a
drop. Save the output as `sonic-as7760-acl-copp-<date>.txt` and add a PROGRESS entry.
