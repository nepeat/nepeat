# Native OpenWrt configuration → BCM56845 hardware

Linux/netifd is authoritative. Configure `swp1`–`swp16`, the default
VLAN-aware `br-switch`, VLAN membership, bridge VLAN interfaces, IPv4 addresses
and routes in the normal LuCI **Network → Interfaces** pages or `/etc/config/network`.
The controller reads actual Linux state, including FDB entries and neighbors,
and applies SDK objects. **Network → Switch hardware** contains only lane
profiles, PHY speed/autonegotiation, status and diagnostics.

## Implementation

- `bcm-network` runs a procd-supervised rtnetlink watcher. Link, bridge VLAN,
  address, neighbor, IPv4 route/rule and forwarding events wake the reconciler
  after a 100 ms debounce (500 ms maximum during a burst). A two-second idle
  resync covers hardware learning, UCI/sysctl changes and dropped notifications.
  Network reload also triggers a reconcile. A broker generation marker forces
  rebuilding after SDK restart. Checked CINT acknowledgements determine success.
- The GPL SDK/TAP adapter transports CPU packets. Factory KNET is compiled out.
  Native Linux admin state wins; SDK reconfiguration cannot change Linux admin
  flags. PHY carrier and speed still come from the SDK.
- The opt-in `950-bcm-sdk-tap-bridge-domain.patch` gives these TAPs a shared
  parent ID and registers their bridge forwarding domain. ASIC CPU copies have
  `offload_fwd_mark`, preventing Linux from re-flooding them to sibling ASIC
  ports. CPU-originated packets still use normal per-port Linux transmission.
  This depends on ASIC forwarding ordinary transit traffic itself; CPU traps
  terminate locally or enter the IP stack. Mixed software/ASIC bridges and
  unsupported flood policies are rejected.
- `native-init.cint` owns and tracks VLANs, local/static MACs, router interfaces,
  next hops and IPv4 LPM routes. Dynamic FDB learning remains in the ASIC;
  Linux static FDB entries are programmed into hardware. Hardware-learned MAC
  locations resolve next hops and supersede stale dynamic Linux FDB entries.
- Resolved IPv4 next hops and connected resolved neighbors can forward in
  hardware. Local addresses, unresolved neighbors, unsupported egress paths
  and less-specific fallbacks point to CPU. Neighbor withdrawal replaces the
  hardware next hop with a CPU route. Linux `ip_forward=0` disables transit
  offload. Policy routing, UCI-configured firewall rules and unsupported ECMP stay
  on the CPU path. A more-specific blackhole is represented by a CPU route
  so a broader hardware prefix cannot bypass the kernel decision.
- Topology/static-FDB changes briefly disable front ports while rebuilding.
  Route/neighbor-only changes retain unchanged router interfaces, next hops
  and prefixes. Changed prefixes use the SDK replace operation, then unused
  routes/next hops/interfaces are collected in dependency order. Unchanged
  PHYs are not rewritten. Object operation counters appear in status.
  A single atomic `native-current.json` publishes the applied plan and status.
  Updates are event-driven with periodic resync and are not atomic; this is a bring-up prototype, unsuitable
  for relying on immediate security-policy enforcement. A rejected configuration
  disables front ports until a supported configuration is restored. Management
  `eth0` remains independent.

## Supported first scope

One VLAN-aware bridge containing only active `swpN` ports; MTU 1500; untagged
PVID and tagged VLAN membership; bridge VLAN IPv4 interfaces; main-table
unicast IPv4 routing. Disable STP and multicast snooping on the bridge.
Unsupported bridge isolation/hairpin/flood-suppression policies fail closed.
Every bridged port needs a PVID. Public VLANs are **1–4030**; **4031–4094** are
reserved for isolated standalone ports (one private VLAN per logical port).

The shipped default is one unaddressed VLAN 1 bridge with all 16 ports,
management DHCP on `eth0`, and 16×40G lanes. Removing a port from the bridge
isolates it in its private VLAN; a native address can then use it directly.
The 64×10G lane profile exists but this validation targets 16×40G. Changing
profiles does not rewrite native network configuration: configure matching
active ports in Network → Interfaces.

Limits enforced by this controller: 128 router interfaces, 1000 next hops and
IPv4 prefixes, 512 local MACs and 512 static FDB entries. IPv6 hardware routing,
VRFs, ACL offload, STP protocol, multicast snooping, jumbo frames, hardware
Linux offload flags and external line-rate forwarding remain unimplemented or
unverified. TAP counters cover CPU traffic, not full hardware traffic. The firewall guard
checks UCI configuration; manually installed nftables/tc rules are not
translated or inspected. This prototype must not be relied on to enforce
arbitrary software packet policies in hardware.

## Inspection

```sh
/etc/init.d/bcm-network status
/usr/libexec/bcm-switch status
ucode /usr/libexec/bcm-native.uc plan
cat /tmp/bcm-sdk/native-status.json
cat /tmp/bcm-sdk/native-plan.json
bcmctl 'vlan show'
bcmctl 'l2 show'
bcmctl 'l3 defip show'
```

Reconcile serialization uses a kernel `flock`, released automatically when a
process dies. The old PID directory remains only as a worker pause marker;
empty, stale and reused-PID markers are repaired after acquiring the real lock.
[Lock recovery tests](lock-tests-20261109.json) cover contention, SIGKILL,
and interruption of a real SDK apply. Before any SDK mutation the controller
writes a pending marker; it clears that marker only after acknowledging the
SDK update and publishing the plan/status. A surviving marker forces a full
rebuild even when Linux intent matches the cached plan. Publication failures
keep the marker, and backend file descriptors use close-on-exec.
`/tmp/bcm-sdk/watcher-status.json` reports event counts and event-triggered
reconcile attempts; status includes `objects` with SDK create/delete/write counters.
A 100 ms wakeup is not an end-to-end completion guarantee: snapshotting and
SDK calls still take time, and updates across multiple tables are not atomic.

Manual `/usr/libexec/bcm-switch reconcile` rebuilds hardware from Linux and can briefly
interrupt front ports. Use native network configuration for persistent intent
within the current RAM boot; arbitrary SDK changes are not authoritative.
No flash or CFE configuration is changed. All settings disappear on power loss.

## Validation

`test_plans.py` exercises 22 semantic fixtures without hardware writes,
including exact default-rule semantics (tables, selectors, duplicate and
missing rules), per-route MTU/link state, policy/firewall/ECMP fallback and
invalid bridge configurations.
`planner-tests-20261109.json` records the results.

`test_hardware.py` changes only native UCI/netifd, FDB and neighbor configuration
for forwarding intent. SDK commands provide MAC loopback, scoped field capture
and readback. Tests check untagged/tagged bridge forwarding, duplicate prevention,
router MAC/TTL/checksum rewrites without Linux ingress, local SVI ARP and stale
next-hop/blackhole negative controls. It restores the native default afterwards.
The final record identifies hot deployment versus a freshly booted image and
includes its artifact manifest. Timestamped records preserve later reruns;
`hardware-result-20261109.json` is the latest run.

The fresh image `1fa0e770d945dd47144527e5f3470aab74e8922c6987e0818b12ab3822b729ba`
passed the packet suite, including 20/20 untagged, 20/20 tagged and 20/20
routed packets, local SVI ARP, neighbor withdrawal and blackhole fallback.
[Automatic startup evidence](fresh-boot-20261109.json) and
[boot capture](boot-20261109.txt) identify the RAM boot.
An initial fresh-boot tagged capture missed its first packet with a 400 ms
receive window; the rerun used a 2 s initial window and passed. The failed
record is retained as `hardware-result-20261109T100954Z.json`; timing remains
a suspected cause, not a proven diagnosis. No forwarding code was changed
between those two runs.

Additional [tagged multicast proof](multicast-result-20261109.json) passed
20/20 packets with zero duplicate/software forwarding on the same RAM image.
The SDK's `Pfm(FloodNone)` display is misleading here: both legacy per-port
PFM getter and setter return `BCM_E_UNAVAIL`; the getter leaves its initial
value unchanged. A trial setter caused the controller to reject the apply
and disable ports, then the original helper was restored and automatic
reconciliation recovered. No PFM override remains. See the
[probe record](multicast-pfm-probe-20261109.json).

[Browser verification](luci-browser-20261109.json) passed both the standard
bridge VLAN filtering editor (swp1–swp16) and the trimmed Switch hardware
page, with no authenticated-view JavaScript exceptions. An unauthenticated
pre-login UCI access rejection is recorded separately.

These are real ASIC MAC-loopback packet tests, not external physical-link or
40G throughput tests. The earlier isolated SDK-only route proof is retained in
[offload/](../offload/README.md).

## Incremental reconciliation follow-up

The [fresh-boot regression](incremental-20261109.json) and earlier hot
regression (`incremental-hot-20261109.json`) exercise
native same-priority policy-rule insertion/removal, 20/20 Linux CPU-forwarded
packets versus 20/20 ASIC-forwarded packets, preserved RIF/CPU-egress objects
across neighbor withdrawal, and exact owned-object counts after each apply.
The watcher counters verify netlink-triggered reconciles occurred. Periodic
resync remains necessary for SDK learning and non-netlink configuration.

The first hot test found a missing joint status file during upgrade, before
changing test intent; the controller now regenerates that file even when the
plan is otherwise unchanged. A later ARP attempt timed out; a focused trace
confirmed request delivery through swp1/bridge/SVI and a Linux reply back on
swp1. The legacy MAC-loopback command silently changes speed to 10G, so the
packet harness now restores 40G after enabling loopback. The subsequent full
hot regression passed. Those failed records remain timestamped; the speed
side effect was observed, but the timeout's exact cause was not established.

Current image: `28b70fcc19199701cfcf1a5379f4f35d3f99778cd8099efa045acce839d0a4e3`.
[Automatic startup and installed-file hashes](fresh-boot-incremental-20261109.json),
[boot transcript](boot-incremental-20261109.txt), and the fresh packet record
confirm the complete image starts without manual backend repairs. It passed
20/20 untagged, tagged and tagged-multicast frames with no duplicates;
20/20 hardware-routed frames with zero Linux ingress; 20/20 CPU-routed frames
under policy fallback; SVI ARP, policy recovery, neighbor withdrawal and
blackhole checks. Object counts match the desired set after reconciliation,
and RIF/CPU-egress objects survive neighbor withdrawal without recreation.
The test restored default VLAN 1 and removed loopbacks/capture/policy rules.

These changes improve reaction and continuity but do not provide atomic
multi-table transactions or a subsecond apply guarantee. The measured policy
transition duration includes host SSH/polling, Linux snapshots and SDK work;
it is recorded in the packet result. STP protocol, IPv6 hardware routing,
full switchdev reporting, and external 40G throughput remain future work.

[Final browser checks](luci-incremental-20261109.json) passed the native bridge
VLAN editor and hardware page after the final image boot.
[Final restored state](final-state-incremental-20261109.json) confirms default
VLAN 1, zero owned L3 objects, both test ports back at 40G without loopback,
and no pending/failed apply marker. Normal periodic snapshots may hold the
apply lock briefly while the watcher remains active.
