/* SPDX-License-Identifier: Apache-2.0
 * openbcm-mdio-script: run MDIO scripts (mdio_script.h) through the switch
 * chip's external MDIO master, for PHYs the SDK has no driver for. Run once
 * per boot, before syncd (NixOS: hardware.openbcm.mdioScript).
 *
 *   openbcm-mdio-script --check SCRIPT...                  parse only (any build)
 *   openbcm-mdio-script --config CONFIG.BCM [--unit N] SCRIPT...
 *
 * Every script is parsed completely before the chip is touched. The SDK is
 * brought up with "init soc" + "init misc" only (sdk_glue.h
 * ob_sdk_init_mdio: the MDIO clock and hold delay are programmed, no port is
 * enabled); the process holds the per-unit SDK lock, so it cannot run next
 * to syncd, bcm.user or openbcm-asic-temp. syncd's later "init all" resets
 * the switch chip, not the PHYs.
 * Exit: 0 done, 1 usage, 2 a script failed on the chip (poll, I/O),
 * 3 SDK lock busy, 4 SDK init failed, 5 script missing or malformed.
 */
#include <getopt.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "mdio_script.h"
#ifdef OB_HAVE_SDK
#include "sdk_glue.h"

static int g_unit;
static int sdk_read(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned* val)
{
    (void)c;
    return ob_sdk_mdio_c45_read(g_unit, phy, dev, reg, val);
}
static int sdk_write(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    (void)c;
    return ob_sdk_mdio_c45_write(g_unit, phy, dev, reg, val);
}

static void log_line(void* c, const char* msg)
{
    (void)c;
    fprintf(stderr, "openbcm-mdio-script: %s\n", msg);
}

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec / 1e9;
}
#endif

static void usage(const char* me)
{
    fprintf(stderr, "usage: %s --check SCRIPT...\n       %s --config CONFIG.BCM [--unit N] SCRIPT...\n", me, me);
}

int main(int argc, char** argv)
{
    static const struct option opts[] = {
        {"check", no_argument, 0, 'c'}, {"config", required_argument, 0, 'f'},
        {"unit", required_argument, 0, 'u'}, {"help", no_argument, 0, 'h'}, {0, 0, 0, 0}};
    int c, i, check = 0, unit = 0, rv;
    const char* config = NULL;
    char err[512] = "";
    mdio_script_stats_t st;

    while ((c = getopt_long(argc, argv, "cf:u:h", opts, NULL)) != -1) {
        switch (c) {
        case 'c': check = 1; break;
        case 'f': config = optarg; break;
        case 'u': unit = atoi(optarg); break;
        default: usage(argv[0]); return c == 'h' ? 0 : 1;
        }
    }
    if (optind >= argc || (!check && !config)) {
        usage(argv[0]);
        return 1;
    }
    for (i = optind; i < argc; i++) {
        rv = mdio_script_run(argv[i], NULL, &st, err, sizeof(err));
        if (rv != MDIO_SCRIPT_OK) {
            fprintf(stderr, "openbcm-mdio-script: %s\n", err);
            if (rv == MDIO_SCRIPT_E_FILE)
                fprintf(stderr, "openbcm-mdio-script: the script is private data that is installed by hand; "
                                "nothing was written to any PHY\n");
            return 5;
        }
        fprintf(stderr, "openbcm-mdio-script: %s: %lu writes, %lu reads (%lu polls), %lu delays (%.1f s), %lu steps\n",
                argv[i], st.writes, st.reads, st.polls, st.delays, (double)st.delay_us / 1e6, st.labels);
    }
    if (check) return 0;
#ifndef OB_HAVE_SDK
    (void)unit;
    fprintf(stderr, "openbcm-mdio-script: built without the SDK: --check only\n");
    return 1;
#else
    {
        mdio_script_io_t io = {sdk_read, sdk_write, NULL, log_line, NULL};
        setvbuf(stdout, NULL, _IOLBF, 0);
        g_unit = unit;
        rv = ob_sdk_init_mdio(unit, config, err, sizeof(err));
        if (rv < 0) {
            fprintf(stderr, "openbcm-mdio-script: %s (%s); nothing was written to any PHY\n", err, ob_sdk_errmsg(rv));
            return rv == OB_E_BUSY ? 3 : 4;
        }
        fprintf(stderr, "openbcm-mdio-script: %s\n", err);
        for (i = optind; i < argc; i++) {
            double t0 = now();
            rv = mdio_script_run(argv[i], &io, &st, err, sizeof(err));
            fprintf(stderr, "openbcm-mdio-script: %s: %s in %.1f s: %lu writes, %lu reads (%lu polls, %lu re-reads), "
                            "%lu expected values differed\n", argv[i], rv == MDIO_SCRIPT_OK ? "done" : "FAILED",
                    now() - t0, st.writes, st.reads, st.polls, st.poll_rereads, st.differs);
            if (rv != MDIO_SCRIPT_OK) {
                fprintf(stderr, "openbcm-mdio-script: %s\n", err);
                return 2;
            }
        }
    }
    return 0;
#endif
}
