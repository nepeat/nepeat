# Hardware forwarding and Linux configuration integration

## Finding, 2026-11-09

**BCM56845_A3 can route in hardware under our OpenWrt/SDK stack.** The earlier
statement that Linux routing uses the CPU describes the current missing
synchronization layer; it is not a hardware restriction or permanent limit
of the SDK/TAP approach.

The live SDK reports 4096 L3 interfaces, 8192 next hops, 8192 host entries,
and 16384 route entries. These are SDK-reported limits for the current table
layout, not tested scale guarantees or promises for mixed IPv4/IPv6 use.
[Live capability query](capabilities-20261109.json).

## Actual packet proof

[Hardware result](hardware-route-result-20261109.json) records this test on
the existing RAM image, SHA-256
`08f7e3d9e96691f0cc9d423d75de6ade85e7f83763c8e00a2b79f97bb0c237b5`.

1. Disable Linux IPv4 forwarding (`ip_forward=0`); no Linux test addresses or
   routes are installed. Create isolated hardware VLANs 4001/4002 on ports
   xe0/xe15, router interfaces 4001/4002, a next-hop object and route
   `198.19.2.0/24` through logical port 16.
2. CPU injects a packet through swp1. Port xe0 MAC loopback makes it ingress
   the ASIC. Hardware performs LPM routing and sends it out xe15.
3. xe15 MAC loopback returns the routed frame to an ingress capture rule
   matching port 16. Copy-to-CPU plus Drop captures it after egress and
   prevents recirculation. SDK/TAP delivers this copy to swp16.
4. **20/20 exact frames passed**: next-hop destination MAC, egress router
   source MAC, TTL 64 → 63, correct updated IPv4 checksum, unchanged payload.
   The actual hardware route's HIT flag changed from n to y.
5. Delete the hardware route and repeat: no routed packet reaches swp16.
   Restore the prior Linux forwarding setting, remove test objects/MACs,
   and restore port/VLAN configuration and 40G defaults.

The CPU generates and observes test traffic; it does not perform the routing
step. This proves hardware L3 forwarding, not external 40G link, throughput,
IPv6 routing, NAT, or full NOS integration. Test configuration was removed;
the normal boot image still has no automatic Linux-route synchronizer.

The first attempt had a route HIT but no completed CPU capture. Adding a
scoped ingress field rule on the egress-loopback port made the forwarding
proof observable. The exact earlier capture failure was not isolated further;
we do not label the first attempt an end-to-end pass. Its record remains in
[attempt 1](hardware-route-attempt1-20261109.json).

## What translates Linux configuration to hardware

SONiC has separate components to create Linux VLAN devices/bridge membership,
consume neighbor changes, and program ASIC objects. These are useful
precedents for a smaller SDK-backed OpenWrt agent:
[SONiC VLAN manager](https://github.com/sonic-net/sonic-swss/blob/master/cfgmgr/vlanmgr.cpp),
[neighbor synchronizer](https://github.com/sonic-net/sonic-swss/blob/master/neighsyncd/neighsync.cpp),
[route/neighbor orchestration overview](https://github.com/sonic-net/SONiC/blob/master/doc/warm-reboot/swss_warm_restart.md).

Proposed state mapping, **not an installed synchronization daemon yet**:

| Linux/OpenWrt state | ASIC state/API |
| --- | --- |
| Bridge VLAN membership, tagging, PVID | `bcm_vlan_*`, port untagged VLAN, ingress filtering |
| Bridge STP and static FDB | STG state and `bcm_l2_addr_*`; SDK learning/age feedback |
| Interface/SVI MAC and VLAN | `bcm_l3_intf_create`; ownership of router MAC classification |
| Local IP addresses | CPU-destination host routes / exception handling |
| Resolved ARP/ND neighbor | `bcm_l3_egress_create`, next-hop MAC/port/interface |
| Selected IPv4 route and withdrawal | `bcm_l3_route_add` / `bcm_l3_route_delete` |

Linux can continue handling FRR, ARP/ND, local traffic, and routing exceptions.
Once resolved, supported transit flows can stay entirely inside the ASIC.
A state agent can consume rtnetlink changes and reconcile through the SDK;
SAI/Redis containers are not a prerequisite for this smaller architecture.

## Integration work that configurations alone do not supply

- Choose one configuration authority. The current Broadcom UCI apply path
  rebuilds VLANs; it must not compete with a Linux-derived state agent.
- Give standalone routed ports isolated internal domains; the current demo's
  default shared VLAN 1 is not Linux-style standalone-port isolation.
- Coordinate router MAC ownership with the TAP adapter's local-MAC-to-CPU
  entries. Local IPs and unresolved neighbors need explicit CPU exceptions.
- Mirror bridge membership, PVID, tagging, STP, FDB changes and aging; map
  Linux routing tables/VRFs and handle neighbor changes before/after routes.
  Route/neighbor withdrawal must remove obsolete forwarding entries.
- Avoid forwarding a CPU copy in Linux after the ASIC already flooded it.
  The Linux switchdev model uses `skb->offload_fwd_mark` and offload-domain
  tracking. Ordinary TAP interfaces do not provide those hooks. A designed
  punt/forwarding policy or thin kernel integration is needed for equivalent
  bridge semantics, not merely copying VLAN membership into hardware.
- Unsupported policy routing, Linux firewall/NAT/conntrack semantics, resource
  limits and SDK errors need defined fallback or rejection. Installing a
  hardware route must not silently bypass a required software policy.
- Reconcile after reboot/restart, and expose applied/failed/offloaded status.
  Generic TAP statistics and Linux offload flags do not automatically report
  ASIC traffic or object programming success.

The kernel already has `CONFIG_BRIDGE=y` and `CONFIG_NET_SWITCHDEV=y`, but
our TAP port devices have no switchdev implementation. This image also lacks
the standalone `bridge` utility (`ip-bridge` package). Installing the utility
provides commands; it does not provide the hardware translation layer.
[Linux bridge/FDB/routing offload semantics](https://docs.kernel.org/networking/switchdev.html).

KNET could improve the host packet path, but it would still need the
configuration/orchestration layer. Its absence did not prevent this ASIC
routing proof. The practical next implementation is a scoped SDK state
agent, initially IPv4 unicast and a constrained bridge/VLAN model, with
explicit synchronization and packet-path tests before expanding features.
