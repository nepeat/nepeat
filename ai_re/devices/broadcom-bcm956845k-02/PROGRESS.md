# Broadcom BCM956845K-02

## Current status

- RAM-netboot OpenWrt 25.12.1 `(emma’s version)`, ASIC BCM56845_A3,
  16×40G profile, LuCI at `https://10.60.2.203/`. Serial:
  `tmux attach -t serial-bcm956845` (9600 8N1).
- Booted image SHA-256:
  `28b70fcc19199701cfcf1a5379f4f35d3f99778cd8099efa045acce839d0a4e3`.
- Native Linux/netifd controls bridges/VLANs/IPv4 routes through the SDK
  controller. Factory KNET is disabled; CPU packet transport uses SDK/TAP.
  CFE/flash are unchanged; all OpenWrt settings are RAM-only.
- [Current configuration, supported scope and evidence](openwrt/native-network/README.md).

## 2026-11-09 — event-driven reconciliation, incremental L3 and crash recovery

- Replaced the polling daemon with a procd rtnetlink watcher: link/VLAN,
  neighbor/address, route/rule and forwarding events trigger debounced applies.
  Periodic resync remains for SDK learning and missed/non-netlink changes.
- L3 updates reuse unchanged RIFs/next hops/routes; changed prefixes use SDK
  replace, then unused objects are collected. Unchanged PHYs are not rewritten.
  Status exposes owned-object counts and SDK operation counters; a single
  atomic JSON snapshot couples the applied plan with its status.
- Tightened default policy-rule semantics, route MTU/encapsulation/link-state
  and inactive egress handling. Replaced PID-directory exclusion with kernel
  flock; empty/stale/reused-PID markers and killed lock owners recover.
- New image built and cold-netbooted; watcher and ASIC initialize automatically,
  and installed backend files match the final image root. 22 planner cases pass.
- Fresh packet regression passed: 20/20 each untagged/tagged/multicast; 20/20
  hardware-routed versus 20/20 CPU-routed during native policy fallback;
  SVI ARP, policy recovery, withdrawal/blackhole and no-leak checks. RIF/CPU
  egress counters prove they survive neighbor withdrawal without recreation.
- An interrupted real SDK apply is also tested: a pending marker survives
  SIGKILL and forces a complete rebuild before accepting cached state.
- Earlier hot upgrade and ARP failures are retained. ARP tracing confirmed
  the full Linux path; the SDK loopback command also silently changed speed
  to 10G, so the test now explicitly restores 40G. Exact timeout cause unproven.
- Native LuCI bridge/VLAN editor and hardware page pass final browser checks.
- Default VLAN 1 restored; test loopbacks, field capture and policy rule removed.
  RAM-only, flash unchanged. Applies still take SDK time and are not atomic;
  STP/IPv6 offload/full switchdev/external 40G testing remain out of scope.
  [Implementation and evidence](openwrt/native-network/README.md).

## 2026-11-09 — native OpenWrt networking programs the ASIC

- Added procd native state controller: Linux bridge/VLAN/PVID, static FDB,
  resolved neighbors and supported IPv4 routes become tracked SDK objects.
  Linux admin state is authoritative. Neighbor withdrawal and native
  blackhole changes replace hardware routes with CPU fallbacks.
- Added an opt-in TAP bridge-domain kernel patch, so ASIC CPU copies are
  not duplicated by Linux bridge flooding. This is a limited SDK/TAP
  integration, not a full DSA/switchdev driver or genuine KNET.
- Removed the separate port-enable/PVID/VLAN editor. Standard LuCI Network
  → Interfaces owns network configuration; **Switch hardware** retains
  lane profiles, PHY settings, status and diagnostics.
- Complete image built and cold-netbooted; ASIC, 16 ports, native bridge
  protection and controller started automatically without manual repair.
- Real browser verification passed the standard bridge VLAN filtering editor
  and trimmed Switch hardware page with no authenticated-view JS exceptions.
- Packet proof using native configuration: **20/20 untagged + 20/20 tagged**
  bridge frames, zero duplicate/software re-forwarding; **20/20 ASIC-routed**
  IPv4 frames with MAC/TTL/checksum rewrites and zero original Linux ingress;
  Linux SVI ARP reply; neighbor-withdrawal and blackhole CPU fallback passed.
  Test configuration/loopbacks/field capture removed; default VLAN 1 restored.
- Additional tagged multicast packet proof passed 20/20 with zero duplicates.
  Legacy PFM getter/setter are unavailable on this ASIC; `FloodNone` in the
  SDK status is misleading. Unsupported override trial reverted, original
  helper restored, default bridge recovered; probe and test recorded.
- One initial fresh-boot tagged capture failed at a 400 ms receive window;
  a 2 s initial receive window passed. Failure retained, cause not proven.
  Fourteen semantic planner cases passed on the fresh image.
- Scope: one VLAN-aware ASIC-only bridge, MTU 1500, IPv4 main-table unicast;
  STP/multicast snooping/unsupported bridge policies rejected. Policy/firewall
  routing uses CPU. Reconciliation is polled and not atomic; Linux hardware
  offload flags, external links/40G throughput and production reliability
  remain unverified. [Details and result records](openwrt/native-network/README.md).


## Earlier investigation

- [Earlier dated journal](HISTORY.md): identity, serial/CFE, firmware dumps,
  factory Linux/ASIC discovery, OpenWrt bring-up, SDK/TAP adapter and route proof.
- [Native configuration and limitations](openwrt/native-network/README.md).
- [Board build and netboot](openwrt/README.md).
- [SDK/TAP packet transport](openwrt/sdk-netdev/README.md).
- [Initial hardware routing research](openwrt/offload/README.md).
- [Peer review](review-20261109.md).
