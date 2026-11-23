// SPDX-License-Identifier: Apache-2.0
// ASIC temperature: SWITCH MAX_NUMBER_OF_TEMP_SENSORS / TEMP_LIST / MAX_TEMP /
// AVERAGE_TEMP (whole degrees C, rounded), and the publisher thread that
// keeps openbcm-asic-temp's files (temp_files.c) up to date while this
// process owns the chip, so fan control keeps its sensor under syncd.
// Profile: SAI_OPENBCM_TEMP_FILE, SAI_OPENBCM_TEMP_PROM, SAI_OPENBCM_TEMP_INTERVAL.
// No over-temperature action here: after "init all" the SDK's own interrupt
// handler is armed.
#include "sai_internal.h"
#include "temp_files.h"

#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdlib>

namespace ob {

bool is_temp_attr(sai_attr_id_t id)
{
    return id == SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS || id == SAI_SWITCH_ATTR_TEMP_LIST ||
           id == SAI_SWITCH_ATTR_MAX_TEMP || id == SAI_SWITCH_ATTR_AVERAGE_TEMP;
}

static int32_t tenths_to_c(double t) { return (int32_t)std::lround(t / 10.0); }

sai_status_t switch_temp_get(uint32_t i, sai_attribute_t& a)
{
    std::vector<int> cur, peak;
    sai_status_t st = ctx().be->asic_temperatures(cur, peak);
    if (st != SAI_STATUS_SUCCESS) return attr_backend_status(st, i);
    if (cur.empty()) return SAI_STATUS_FAILURE;
    long sum = 0;
    int hot = ob_temp_hottest(cur.data(), (int)cur.size(), &sum);
    switch (a.id) {
    case SAI_SWITCH_ATTR_MAX_NUMBER_OF_TEMP_SENSORS: a.value.u8 = (uint8_t)cur.size(); return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_TEMP_LIST: {
        std::vector<int32_t> v;
        for (int t : cur) v.push_back(tenths_to_c(t));
        return fill_list(a.value.s32list, v);
    }
    case SAI_SWITCH_ATTR_MAX_TEMP: a.value.s32 = tenths_to_c(cur[hot]); return SAI_STATUS_SUCCESS;
    case SAI_SWITCH_ATTR_AVERAGE_TEMP: a.value.s32 = tenths_to_c((double)sum / (double)cur.size()); return SAI_STATUS_SUCCESS;
    default: return attr_not_supported(i);
    }
}

// --------------------------------------------------------------- publisher
namespace {
struct TempPub {
    std::mutex mu;                  // guards stop and the files
    std::condition_variable cv;
    bool stop = false;
    std::thread th;
    std::string out, prom;
    int unit = 0, interval = 5;
    Backend* be = nullptr;
};
TempPub& tp()
{
    static TempPub* p = new TempPub();   // never destroyed (atexit handler, detached thread)
    return *p;
}
const char* cstr(const std::string& s) { return s.empty() ? nullptr : s.c_str(); }
} // namespace

static void temp_loop()
{
    auto& t = tp();
    bool good_before = false, first = true, write_warned = false;
    std::unique_lock<std::mutex> lk(t.mu);
    while (!t.stop) {
        lk.unlock();
        std::vector<int> cur, peak;
        sai_status_t st = t.be->asic_temperatures(cur, peak);
        lk.lock();
        if (t.stop) break;
        int n = (int)cur.size();
        if (st == SAI_STATUS_SUCCESS && peak.size() == cur.size() && ob_temp_plausible(cur.data(), n)) {
            int w = ob_temp_write(cstr(t.out), cstr(t.prom), t.unit, cur.data(), peak.data(), n);
            if (w && !write_warned)
                OB_WARN(SAI_API_SWITCH, "ASIC temperature: cannot write %s: %s",
                        (w & OB_TEMP_W_OUT) ? t.out.c_str() : t.prom.c_str(), std::strerror(errno));
            write_warned = w != 0;
            if (!good_before) {
                int hot = ob_temp_hottest(cur.data(), n, nullptr);
                OB_NOTICE(SAI_API_SWITCH, "ASIC temperature: %d monitors, max %.1f C; every %d s to %s%s%s", n,
                          cur[hot] / 10.0, t.interval, t.out.empty() ? "-" : t.out.c_str(),
                          t.prom.empty() ? "" : " and ", t.prom.c_str());
            }
            good_before = true;
        } else {
            ob_temp_remove(cstr(t.out), cstr(t.prom));   // no reading, not an old one
            if (good_before || first)
                OB_WARN(SAI_API_SWITCH, "ASIC temperature read failed (status %d, %d monitors%s); files removed", st,
                        n, st == SAI_STATUS_SUCCESS ? ", implausible value" : "");
            good_before = false;
        }
        first = false;
        t.cv.wait_for(lk, std::chrono::seconds(t.interval), [&] { return t.stop; });
    }
    ob_temp_remove(cstr(t.out), cstr(t.prom));
}

// exit() without remove_switch: no stale reading for fan control. Does not
// join (the thread may sit in an SDK read); it checks stop under the same
// lock before every write.
static void temp_atexit()
{
    auto& t = tp();
    std::lock_guard<std::mutex> g(t.mu);
    if (!t.th.joinable()) return;
    t.stop = true;
    ob_temp_remove(cstr(t.out), cstr(t.prom));
    t.cv.notify_all();
}

void temp_publisher_start(const BackendConfig& cfg, Backend* be)
{
    auto& t = tp();
    std::string out = cfg.get("SAI_OPENBCM_TEMP_FILE"), prom = cfg.get("SAI_OPENBCM_TEMP_PROM");
    if (out.empty() && prom.empty()) return;
    int interval = std::atoi(cfg.get("SAI_OPENBCM_TEMP_INTERVAL", "5").c_str());
    if (interval < 1 || interval > 3600) {
        OB_WARN(SAI_API_SWITCH, "SAI_OPENBCM_TEMP_INTERVAL '%s' out of 1..3600; using 5",
                cfg.get("SAI_OPENBCM_TEMP_INTERVAL").c_str());
        interval = 5;
    }
    static std::once_flag once;
    std::call_once(once, [] { std::atexit(temp_atexit); });
    std::lock_guard<std::mutex> g(t.mu);
    if (t.th.joinable()) return;
    t.out = out;
    t.prom = prom;
    t.unit = std::atoi(cfg.get("SAI_OPENBCM_UNIT", "0").c_str());
    t.interval = interval;
    t.be = be;
    t.stop = false;
    t.th = std::thread(temp_loop);
}

void temp_publisher_stop()
{
    auto& t = tp();
    {
        std::lock_guard<std::mutex> g(t.mu);
        if (!t.th.joinable()) return;
        t.stop = true;
    }
    t.cv.notify_all();
    t.th.join();
    std::lock_guard<std::mutex> g(t.mu);
    ob_temp_remove(cstr(t.out), cstr(t.prom));
    t.be = nullptr;
}

} // namespace ob
