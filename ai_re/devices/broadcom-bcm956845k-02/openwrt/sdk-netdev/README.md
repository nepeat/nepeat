# Experimental SDK-backed Linux port devices

The factory SDK 5.9.1 was built **without KNET**. On the unmodified SDK,
`bcm_knet_init(0)` returns `-16` (BCM_E_UNAVAIL); disassembly confirms
`bcm_esw_knet_init`, netif creation and filter APIs are literal two-instruction
return-unavailable stubs. API names in CINT do not prove implementation.

Broadcom SONiC uses KNET: see its
[syncd setup](https://github.com/sonic-net/sonic-sairedis/blob/master/syncd/scripts/syncd_init_common.sh)
and Broadcom's
[KNET driver](https://github.com/Broadcom-Network-Switching-Software/OpenBCM/blob/master/sdk-6.5.16/systems/linux/kernel/modules/bcm-knet/bcm-knet.c).
A genuine KNET port here needs a compatible SDK build with KNET enabled,
plus kernel/BDE integration. This adapter does **not** implement that ABI.

## Implemented approach

Our own small GPL adapter uses the existing SDK's working packet TX/RX APIs
and Linux TAP devices. The interfaces are real Linux Ethernet devices, not
dummy interfaces or UI-only placeholders. Linux IP configuration and packet
sockets use them normally; hardware switching remains SDK-controlled.

- `swp1` maps to SDK `xe0` / logical port 1, through `swp16` / `xe15` in 40G mode.
  These are logical names; physical front-panel jack numbering is unverified.
- The 10G profile creates 64 devices; current validation concentrates on 40G.
- The worker passes TX frames to `bcm_tx` and routes CPU-bound RX by ingress
  port to the corresponding TAP. ASIC-switched transit traffic stays in ASIC.
- Physical carrier and current SDK speed are reflected into Linux. Admin
  changes propagate between Linux and SDK. Speed configuration remains on the
  ASIC page/UCI; TAP ethtool settings alone do not configure hardware.
- Each Linux MAC gets a static L2 destination to CPU port 0 in that port's
  PVID. CPU membership must be allowed in the configured VLAN.
- `/tmp/bcm-sdk/netdev-status.json` identifies backend `sdk-tap`, explicitly
  reports `knet:false`, and records per-device CPU-path packet/error counts.
- Linux/TAP statistics measure CPU traffic, not all front-panel hardware traffic.
- 1500-byte MTU is the supported starting configuration; jumbo support is untested.

Native LuCI Network → Devices and normal interface creation can use `swpN`.
Adding an IP/interface configuration does not automatically program ASIC
VLANs or routing. Use Network → Broadcom ASIC for hardware port/VLAN settings.
The baseline has no automatic route synchronizer. Hardware IPv4 routing
was subsequently proven with Linux forwarding disabled; see the
[hardware offload findings and test](../offload/README.md).
**Linux bridge/VLAN changes are not synchronized to ASIC tables.** Do not
assume DSA/switchdev behavior or combine software bridges with overlapping
hardware switching domains without an explicit forwarding design.

## Exact-binary adapter mechanism

The original SDK hash is pinned in `patch_sdk.py`. A freestanding PowerPC
payload is linked at `0x11400000`, above all existing SDK load segments.
One optional ELF PT_NOTE header becomes an extra PT_LOAD for our code/data.
The original disabled `bcm_esw_knet_init` stub is replaced by a relative branch
used **only as an explicit adapter bootstrap** from `start-tap.cint`.
Other KNET APIs remain unavailable. This is an experimental binary extension,
not a claim that stock KNET has been enabled. No existing SDK text is moved;
original bytes and branch range are checked before patching.

SDK call addresses come from the hash-checked ELF symbol table. Packet and
L2 structure offsets were measured through the running SDK's CINT metadata;
these are specific to SDK 5.9.1, including 12-byte port bitmaps. Calls into
old glibc use an explicit 32-bit timeout ABI, independent of modern musl's
time64 layout. The new adapter is source-built; proprietary SDK binaries
remain in ignored local build output.

`prepare_sdk.py` builds/stages this adapter by default. Set
`OPENWRT_SDK_TAP=0` for an ASIC-only image using only the earlier pathname
patch. The build adds `kmod-tun`; CFE and flash remain untouched.

## Bring-up findings

- SDK packet allocation already accounts for CRC append; adding extra space
  padded payloads by four bytes. Exact frame tests caught and corrected this.
- Wait for carrier and vendor linkscan after MAC-loopback transitions. The
  SDK loopback path can report 10G even under the 40G lane profile; internal
  loopback results do not prove external 40G operation.
- Linux protocol-specific packet sockets do not capture all outgoing kernel
  replies. ARP verification uses ETH_P_ALL and checks the generated reply.
- Native LuCI chains the DHCP UCI config even without a DHCP server package.
  Empty DHCP/firewall config files avoid a spurious native-page RPC error.

## Hardware testing

`test_packet.c` requires explicit MAC loopback on the corresponding ASIC
port; it checks exact frame length/content after the complete Linux → SDK
DMA → ASIC MAC → SDK RX → Linux round trip. `test_arp.c` requires a temporary
198.18.0.1/30 address and checks a Linux-generated ARP response to a request
sent through ASIC loopback. These are internal tests, not external cable,
40G throughput, or production-stability validation.

## Validated image, 2026-11-09

[Hash-linked hardware result](hardware-result-20261109.json) and
[cold boot capture](boot-20261109.txt). Netboot image SHA-256:
`08f7e3d9e96691f0cc9d423d75de6ade85e7f83763c8e00a2b79f97bb0c237b5`.

Fresh boot automatically created all 16 devices. Linux admin down/up was
verified against ASIC readback. Ports `swp1` and `swp16` each passed 20/20
exact Ethernet frame round trips through ASIC MAC loopback. A temporary
netifd static interface at 198.18.0.1/30 generated the expected Linux ARP
reply. The authenticated native LuCI Interfaces page displayed the static
interface and all 16 devices without view JS exceptions. The test records
unauthenticated login-page RPC refusals separately from the actual view.
Temporary IP and loopbacks were removed; all 16 ASIC ports and Linux speed
readback returned to 40G, with carrier absent on disconnected ports.

ARP request/reply, IPv6 ND and DHCP CPU-copy controls are enabled through SDK
APIs (all returned success); only ARP has an end-to-end protocol test so far.
All state remains RAM-only. No external cable/peer or throughput test was run.
