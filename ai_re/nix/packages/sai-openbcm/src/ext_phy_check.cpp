// SPDX-License-Identifier: Apache-2.0
// openbcm-ext-phy --check FILE: parse an external PHY table (ext_phy.h) as
// libsai will and print what it holds. For build-time checks of generated
// board tables: a table libsai refuses means no switch at boot.
#include "ext_phy.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <sstream>

int main(int argc, char** argv)
{
    if (argc != 3 || std::strcmp(argv[1], "--check") != 0) {
        std::fprintf(stderr, "usage: %s --check FILE\n", argv[0]);
        return 2;
    }
    std::ifstream f(argv[2]);
    if (!f) {
        std::fprintf(stderr, "%s: cannot open\n", argv[2]);
        return 1;
    }
    std::stringstream ss;
    ss << f.rdbuf();
    ob::ExtPhyTable t;
    std::string err;
    if (!ob::ext_phy_parse(ss.str(), t, err)) {
        std::fprintf(stderr, "%s: %s\n", argv[2], err.c_str());
        return 1;
    }
    size_t link = 0, down = 0, up = 0, nl = 0;
    for (auto& p : t.ports) {
        link += p.link.size();
        down += p.down.size();
        up += p.up.size();
        nl += !p.link.empty();
    }
    std::printf("%s: %zu ports (%zu with a link poll), poll %u ms, up after %u good polls, hold-off %u ms; "
                "operations: link %zu, admin-down %zu, admin-up %zu\n",
                argv[2], t.ports.size(), nl, t.poll_ms, t.up_polls, t.holdoff_ms, link, down, up);
    return 0;
}
