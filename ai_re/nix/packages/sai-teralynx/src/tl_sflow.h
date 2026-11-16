// SPDX-License-Identifier: Apache-2.0
// sFlow samples to the host's psample (generic-netlink host interfaces).
//
// SONiC asks for a SAI_HOSTIF_TYPE_GENETLINK host interface ("psample",
// multicast group "packets") and a HOSTIF_TABLE_ENTRY (TRAP_ID = the
// SAMPLEPACKET trap, channel GENETLINK) sending the trap's packets there.
// IFCS 0.12.41 has no netlink API (0.16 has ifcs_netlink_create /
// netlink_trapid_add); the ipd driver under it does: IPD_CREATE_NETLINK
// (nl_app_type SFLOW) makes the driver recognise the chip's sample frames
// (Ethernet / IPv4 / GRE with the protocol set by IPD_SET_PARAMS, the 16 B
// sFlow header with the ingress / egress sysport, the sampled frame), strip
// them and send them as psample messages (in / out ifindex of the sysports'
// netdevs, the port's sample rate from IPD_SET_NETDEV
// INNO_ND_*_SFLOW_SAMPLE_RATE, the frame). With the driver parameter
// psample=1 (our ipd-sflow-psample.patch) the messages go through the
// kernel's own psample (groups 1 ingress / 2 egress), else through a family
// the driver registers under the hostif's name.
//
// These ioctls go to the /dev/ipd file libifcs opened in this process (the
// driver refuses a second open); found through /proc/self/fd. A libifcs
// exporting tl_ipd_ioctl (the test mock) gets them instead.
//
//  - genetlink_create records the name and group; nothing reaches the driver
//    until the table entry for SAMPLEPACKET exists (SAI: no entry, no
//    delivery; the samples keep going to the ingress port's netdev);
//  - genetlink_trap_set(SAMPLEPACKET, on): SET_PARAMS + CREATE_NETLINK, then
//    every sampled port's rate; off: DELETE_NETLINK. Other trap types:
//    NOT_SUPPORTED. One sFlow netlink at a time (the driver has one);
//  - port_rate (from MirrorArea on every sampler change): the port's rate
//    for the samples' metadata, pushed while a netlink is active. A driver
//    without the rate ioctl in legacy mode (no patch) refuses it: logged
//    once, the samples then carry rate 0.
// The driver drops its netlinks when libifcs closes /dev/ipd; shutdown()
// deletes them first (switch re-create in one process).
#pragma once

#include "tl_core.h"

namespace ob {
namespace tlb {

class SflowArea {
public:
    explicit SflowArea(Core& c) : c_(c) {}

    sai_status_t genetlink_create(const std::string& name, const std::string& mcgrp, int& id);
    sai_status_t genetlink_destroy(int id);
    sai_status_t genetlink_trap_set(int id, int32_t trap_type, bool on);
    void port_rate(int hw, bool ingress, uint32_t rate);   // 0: sampler off
    void shutdown();
    std::string show();                                    // part of diag "mirror"

    // The chip's sample frames: GRE protocol of the sFlow collector's
    // encapsulation (seen on the chip with isai's collector tags).
    static constexpr uint16_t SHIM_GRE_PROTO = 0x8818;

private:
    struct Genl { std::string name, mcgrp; bool active = false; };
    using IoctlFn = int (*)(int, unsigned long, void*);

    bool ipd_ready();
    int ipd(unsigned nr, void* arg, const char* what);     // 0 or errno
    sai_status_t activate(Genl& g);
    void deactivate(Genl& g);
    int push_rate(int hw, bool ingress, uint32_t rate);

    Core& c_;
    int next_ = 1;
    std::map<int, Genl> genl_;
    std::map<std::pair<int, bool>, uint32_t> rates_;        // (hw, ingress) -> 1 in N
    IoctlFn ioctl_ = nullptr;
    int fd_ = -1;
    bool resolved_ = false;
    unsigned rate_errors_ = 0;
};

} // namespace tlb
} // namespace ob
