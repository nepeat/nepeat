// SPDX-License-Identifier: Apache-2.0
// oracle-prep: a sai-teralynx oracle JSON script (oracle/scripts, read only) made runnable on the mock
// libifcs (crate sai-teralynx-parity, README there). Only the SAI steps are kept:
//   - "exec" steps are dropped (frame senders / counters on the box's netdevs, teamd, ...: they need
//     the chip and real traffic; their SAI effects are none);
//   - "sleep" steps wait 0 ms (they wait for links, learning or traffic on the box; the mock answers
//     at once, and nothing in the stream depends on the clock).
// Everything else is copied as it is, nested steps ("foreach", "repeat") included.
//   oracle-prep IN.json OUT.json   prints "<n> exec dropped, <n> sleeps zeroed" and one line per
//                                  dropped exec ("exec <cmd>")
#include <nlohmann/json.hpp>

#include <cstdio>
#include <fstream>
#include <iostream>
#include <string>

using json = nlohmann::json;

static int g_exec = 0, g_sleep = 0;

static json prep(const json& steps)
{
    json out = json::array();
    for (const auto& s : steps) {
        std::string op = s.value("op", std::string());
        if (op == "exec") {
            g_exec++;
            std::printf("exec %s\n", s.value("cmd", std::string()).c_str());
            continue;
        }
        json t = s;
        if (op == "sleep") {
            g_sleep++;
            t["ms"] = 0;
        }
        if (t.contains("steps") && t["steps"].is_array()) t["steps"] = prep(t["steps"]);
        out.push_back(t);
    }
    return out;
}

int main(int argc, char** argv)
{
    if (argc != 3) {
        std::fprintf(stderr, "usage: %s IN.json OUT.json\n", argv[0]);
        return 2;
    }
    json script;
    try {
        std::ifstream in(argv[1]);
        in >> script;
    } catch (std::exception& e) {
        std::fprintf(stderr, "%s: %s\n", argv[1], e.what());
        return 2;
    }
    if (!script.contains("steps") || !script["steps"].is_array()) {
        std::fprintf(stderr, "%s: no steps\n", argv[1]);
        return 2;
    }
    script["steps"] = prep(script["steps"]);
    std::ofstream(argv[2]) << script.dump(1) << "\n";
    std::printf("%d exec dropped, %d sleeps zeroed\n", g_exec, g_sleep);
    return 0;
}
