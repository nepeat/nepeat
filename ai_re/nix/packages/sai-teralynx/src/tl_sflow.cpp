// SPDX-License-Identifier: Apache-2.0
// sFlow samples to psample through the ipd driver's netlink ioctls
// (tl_sflow.h). The ioctl argument layouts are the driver's (inno_ioctl.h of
// ipd 0.16.2, GPL; mrvl-teralynx), restated here for interop only.
#include "tl_sflow.h"

#include <dirent.h>
#include <dlfcn.h>
#include <sys/ioctl.h>
#include <unistd.h>

#include <cerrno>
#include <cstdio>

namespace ob {
namespace tlb {

namespace {

// ipd ioctls: _IOWR('I', nr, <pointer>); the driver dispatches on nr only.
constexpr unsigned NR_SET_NETDEV = 26, NR_CREATE_NETLINK = 27, NR_DELETE_NETLINK = 28, NR_SET_PARAMS = 31;
constexpr unsigned long ipd_cmd(unsigned nr) { return _IOWR('I', nr, void*); }
constexpr uint16_t IOCTL_VERSION = 1;
constexpr uint32_t NETLINK_GENERIC_TYPE = 16;          // NETLINK_GENERIC
constexpr uint32_t APP_SFLOW = 1;                       // INNO_NL_SFLOW
constexpr uint16_t ND_INGRESS_RATE = 8, ND_EGRESS_RATE = 9;   // INNO_ND_*_SFLOW_SAMPLE_RATE

struct Hdr {            // inno_ioctl_hdr_t, 8 B
    uint16_t version;
    uint8_t instance, unused1;
    uint32_t unused2;
};
struct Netlink {        // inno_ioctl_netlink_t, 52 B
    Hdr hdr;
    uint32_t type;
    uint32_t app;
    char name[17];      // IFNAMSIZ + 1
    char mcgrp[16];     // GENL_NAMSIZ
};
struct Params {         // inno_ioctl_params_t, 12 B
    Hdr hdr;
    uint16_t shim_gre_proto, rsvd;
};
// inno_ioctl_netdev_info_t: packed {hdr, u16 id, u16 sysport, union}; the
// rate is the union's first u32 in both the 0.12.x (76 B) and the 0.16
// (140 B) layout, so one zeroed 140 B buffer serves either driver mode.
constexpr size_t NETDEV_INFO_SIZE = 140, NETDEV_INFO_U = 12;
static_assert(sizeof(Hdr) == 8 && sizeof(Netlink) == 52 && sizeof(Params) == 12, "ipd ioctl layouts (inno_ioctl.h)");

Hdr hdr() { Hdr h{}; h.version = IOCTL_VERSION; return h; }

void copy_name(char* dst, size_t n, const std::string& s)
{
    std::memset(dst, 0, n);
    std::memcpy(dst, s.data(), std::min(s.size(), n - 1));
}

int real_ioctl(int fd, unsigned long cmd, void* arg) { return ::ioctl(fd, cmd, arg); }

} // namespace

bool SflowArea::ipd_ready()
{
    if (!resolved_) {
        resolved_ = true;
        if (c_.L.handle) ioctl_ = reinterpret_cast<IoctlFn>(dlsym(c_.L.handle, "tl_ipd_ioctl"));
        if (ioctl_) return true;   // the test mock: no device
        ioctl_ = real_ioctl;
    }
    if (ioctl_ != real_ioctl) return true;
    if (fd_ >= 0) return true;
    // the /dev/ipd libifcs opened (legacy_abi: /dev/ipd, else /dev/ipd0)
    std::string want = c_.key("SAI_TERALYNX_IPD_DEV", "");
    DIR* d = opendir("/proc/self/fd");
    if (!d) return false;
    while (dirent* e = readdir(d)) {
        char link[300], target[256];
        std::snprintf(link, sizeof(link), "/proc/self/fd/%s", e->d_name);
        ssize_t n = readlink(link, target, sizeof(target) - 1);
        if (n <= 0) continue;
        target[n] = 0;
        std::string t = target;
        if (want.empty() ? (t == "/dev/ipd" || t == "/dev/ipd0") : t == want) {
            fd_ = std::atoi(e->d_name);
            break;
        }
    }
    closedir(d);
    if (fd_ < 0) c_.log_once("sflow:nofd", "sflow: no open /dev/ipd in this process (libifcs not up?)");
    return fd_ >= 0;
}

int SflowArea::ipd(unsigned nr, void* arg, const char* what)
{
    if (!ipd_ready()) return ENODEV;
    if (ioctl_(fd_, ipd_cmd(nr), arg) == 0) return 0;
    int err = errno ? errno : EIO;
    if (err == EBADF) fd_ = -1;   // look again next time
    OB_WARN(SAI_API_HOSTIF, "sflow: ipd %s failed: %s", what, std::strerror(err));
    return err;
}

sai_status_t SflowArea::genetlink_create(const std::string& name, const std::string& mcgrp, int& id)
{
    Guard g(c_.mu);
    if (name.empty() || name.size() > 15 || mcgrp.empty() || mcgrp.size() > 15) return SAI_STATUS_INVALID_PARAMETER;
    id = next_++;
    genl_[id] = Genl{name, mcgrp, false};
    return SAI_STATUS_SUCCESS;
}

sai_status_t SflowArea::genetlink_destroy(int id)
{
    Guard g(c_.mu);
    auto it = genl_.find(id);
    if (it == genl_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (it->second.active) deactivate(it->second);
    genl_.erase(it);
    return SAI_STATUS_SUCCESS;
}

sai_status_t SflowArea::genetlink_trap_set(int id, int32_t trap_type, bool on)
{
    Guard g(c_.mu);
    auto it = genl_.find(id);
    if (it == genl_.end()) return SAI_STATUS_ITEM_NOT_FOUND;
    if (trap_type != SAI_HOSTIF_TRAP_TYPE_SAMPLEPACKET) {
        c_.log_once("sflow:trap", "sflow: only SAMPLEPACKET packets can go to a genetlink host interface");
        return SAI_STATUS_NOT_SUPPORTED;
    }
    Genl& gl = it->second;
    if (!on) {
        if (gl.active) deactivate(gl);
        return SAI_STATUS_SUCCESS;
    }
    if (gl.active) return SAI_STATUS_SUCCESS;
    for (auto& o : genl_)
        if (o.second.active) return SAI_STATUS_ITEM_ALREADY_EXISTS;   // the driver has one sFlow netlink
    return activate(gl);
}

sai_status_t SflowArea::activate(Genl& gl)
{
    Params p{};
    p.hdr = hdr();
    p.shim_gre_proto = SHIM_GRE_PROTO;
    int err = ipd(NR_SET_PARAMS, &p, "SET_PARAMS (sample GRE protocol)");
    if (err) return err == ENODEV ? SAI_STATUS_UNINITIALIZED : SAI_STATUS_FAILURE;
    Netlink n{};
    n.hdr = hdr();
    n.type = NETLINK_GENERIC_TYPE;
    n.app = APP_SFLOW;
    copy_name(n.name, sizeof(n.name), gl.name);
    copy_name(n.mcgrp, sizeof(n.mcgrp), gl.mcgrp);
    if ((err = ipd(NR_CREATE_NETLINK, &n, "CREATE_NETLINK"))) return SAI_STATUS_FAILURE;
    gl.active = true;
    rate_errors_ = 0;
    for (auto& r : rates_) push_rate(r.first.first, r.first.second, r.second);
    OB_NOTICE(SAI_API_HOSTIF, "sflow: samples to genetlink %s / %s (%zu sampled ports)", gl.name.c_str(),
              gl.mcgrp.c_str(), rates_.size());
    return SAI_STATUS_SUCCESS;
}

void SflowArea::deactivate(Genl& gl)
{
    Netlink n{};
    n.hdr = hdr();
    n.type = NETLINK_GENERIC_TYPE;
    n.app = APP_SFLOW;
    copy_name(n.name, sizeof(n.name), gl.name);
    copy_name(n.mcgrp, sizeof(n.mcgrp), gl.mcgrp);
    ipd(NR_DELETE_NETLINK, &n, "DELETE_NETLINK");
    gl.active = false;
}

int SflowArea::push_rate(int hw, bool ingress, uint32_t rate)
{
    auto sp = c_.sysport.find(hw);
    if (sp == c_.sysport.end()) return EINVAL;
    uint8_t b[NETDEV_INFO_SIZE] = {};
    Hdr h = hdr();
    uint16_t id = ingress ? ND_INGRESS_RATE : ND_EGRESS_RATE;
    uint16_t sysport = (uint16_t)IFCS_HANDLE_VALUE(sp->second);
    std::memcpy(b, &h, sizeof(h));
    std::memcpy(b + 8, &id, 2);
    std::memcpy(b + 10, &sysport, 2);
    std::memcpy(b + NETDEV_INFO_U, &rate, 4);
    int err = ipd(NR_SET_NETDEV, b, "SET_NETDEV sample rate");
    if (err && !rate_errors_++)
        OB_WARN(SAI_API_HOSTIF, "sflow: the driver refuses sample rates (ipd without ipd-sflow-psample.patch in "
                                "legacy_abi mode?): samples carry rate 0");
    return err;
}

void SflowArea::port_rate(int hw, bool ingress, uint32_t rate)
{
    Guard g(c_.mu);
    auto key = std::make_pair(hw, ingress);
    if (rate) rates_[key] = rate;
    else if (!rates_.erase(key)) return;
    for (auto& o : genl_)
        if (o.second.active) { push_rate(hw, ingress, rate); break; }
}

void SflowArea::shutdown()
{
    Guard g(c_.mu);
    for (auto& o : genl_)
        if (o.second.active) deactivate(o.second);
    rates_.clear();
    fd_ = -1;
}

std::string SflowArea::show()
{
    Guard g(c_.mu);
    std::string s;
    char b[200];
    for (auto& o : genl_) {
        std::snprintf(b, sizeof(b), "sflow: genetlink %s / %s %s, %zu sampled ports, rate errors %u\n", o.second.name.c_str(),
                      o.second.mcgrp.c_str(), o.second.active ? "active (SAMPLEPACKET)" : "idle", rates_.size(), rate_errors_);
        s += b;
    }
    if (genl_.empty()) s = "sflow: no genetlink host interface\n";
    return s;
}

} // namespace tlb
} // namespace ob
