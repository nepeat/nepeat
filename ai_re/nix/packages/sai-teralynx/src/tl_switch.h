// SPDX-License-Identifier: Apache-2.0
// Switch: profile, SDK init / shutdown, init commands, event callbacks,
// temperatures, table usage, chip limits, the read-only diag shell.
#pragma once

#include "tl_core.h"

#include <chrono>

namespace ob {
namespace tlb {

class SwitchArea {
public:
    explicit SwitchArea(Core& c) : c_(c) {}

    sai_status_t init(const BackendConfig& cfg, LinkCallback link_cb, FdbCallback fdb_cb);
    void shutdown();
    sai_status_t run_shell(const std::string& cmd);
    sai_status_t run_shell_to(const std::string& cmd, int fd);
    std::string hardware_info() { return "teralynx7 ifcs-0.12.41"; }
    sai_status_t asic_temperatures(std::vector<int>& cur, std::vector<int>& peak);
    sai_status_t table_usage(TableUsage& u);
    sai_status_t chip_limits(ChipLimits& l);

private:
    sai_status_t post_enable(const tl::InitCmds& ic);
    bool apply_cmd(const tl::DevportCmd& c);
    void deinit();
    sai_status_t shell(const std::string& cmd, std::string& out);
    static void on_log(int level, void*, const char* msg);

    Core& c_;
    bool deinit_on_shutdown_ = true;
    std::string run_config_;
    bool limits_read_ = false;
    ChipLimits limits_;
    // temperature cache (asic_temperatures)
    std::mutex temp_mu_;
    int temp_period_ = -1, temp_n_ = 5, temp_next_ = 0;
    unsigned temp_reads_ = 0;
    bool temp_valid_[5] = {false, false, false, false, false};
    int temp_tenths_[5] = {0, 0, 0, 0, 0};
    std::chrono::steady_clock::time_point temp_when_;
};

} // namespace tlb
} // namespace ob
