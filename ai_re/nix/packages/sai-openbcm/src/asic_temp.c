/* SPDX-License-Identifier: Apache-2.0
 * openbcm-asic-temp: read the switch ASIC's die temperature monitors through
 * the Broadcom SDK and publish them for fan control and monitoring.
 *
 * Attaches only (sdk_glue.c ob_sdk_attach): no chip reset, no "init all",
 * ports and forwarding untouched (on a never-initialised chip it programs
 * the S-bus ring map and PVTMON trim first, as the SDK's chip reset does).
 * It holds the per-unit SDK lock while it runs, so bcm.user / syncd cannot
 * start next to it (they would share the DMA pool and the S-channel): stop
 * it first.
 *
 *   openbcm-asic-temp --once                 print the monitors, exit
 *   openbcm-asic-temp -o FILE [-p PROM] [-i SECONDS] [-u UNIT] [-s C]
 *     FILE / PROM: see temp_files.h (the same writer libsai uses under
 *     syncd). Both are removed on exit and after a failed read, so a
 *     consumer sees "no reading" rather than an old one. Exits 4 after 3
 *     failed reads in a row.
 *     --shutdown-at C (default 125, 0 = off): over-temperature protection.
 *     Only "init all" arms the chip's own (soc_tomahawk_temperature_intr,
 *     SDK default 125 C), and this reader never runs "init all". When any
 *     current monitor is at or above C on 2 consecutive good reads, it
 *     powers down the core PLL exactly as that handler does
 *     (ob_sdk_th_overtemp_shutdown), removes both files and exits 5. The
 *     chip then needs a power cycle. Not in --once mode.
 */
#include <errno.h>
#include <getopt.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include "sdk_glue.h"
#include "temp_files.h"

#define MAXMON OB_TEMP_MAX_MON

static volatile sig_atomic_t stop_flag;
static void on_signal(int s) { (void)s; stop_flag = 1; }

static void usage(const char* me)
{
    fprintf(stderr, "usage: %s [--unit N] (--once | --out FILE [--prom FILE] [--interval SECONDS] "
                    "[--shutdown-at CELSIUS (0 = off, default 125)])\n", me);
}

int main(int argc, char** argv)
{
    static const struct option opts[] = {
        {"unit", required_argument, 0, 'u'}, {"interval", required_argument, 0, 'i'},
        {"out", required_argument, 0, 'o'}, {"prom", required_argument, 0, 'p'},
        {"shutdown-at", required_argument, 0, 's'}, {"once", no_argument, 0, '1'},
        {"help", no_argument, 0, 'h'}, {0, 0, 0, 0}};
    int unit = 0, interval = 5, once = 0, c, rv, fails = 0, first = 1, shutdown_at = 125, hot_reads = 0;
    const char *out = NULL, *prom = NULL;
    char err[256] = "";
    struct sigaction sa;

    while ((c = getopt_long(argc, argv, "u:i:o:p:s:1h", opts, NULL)) != -1) {
        switch (c) {
        case 'u': unit = atoi(optarg); break;
        case 'i': interval = atoi(optarg); break;
        case 'o': out = optarg; break;
        case 'p': prom = optarg; break;
        case 's': shutdown_at = atoi(optarg); break;
        case '1': once = 1; break;
        default: usage(argv[0]); return c == 'h' ? 0 : 1;
        }
    }
    if ((!once && !out) || interval < 1 || interval > 3600 || shutdown_at < 0 || shutdown_at > 150) {
        usage(argv[0]);
        return 1;
    }
    setvbuf(stdout, NULL, _IOLBF, 0);

    rv = ob_sdk_attach(unit, err, sizeof(err));
    if (rv < 0) {
        fprintf(stderr, "openbcm-asic-temp: %s (%s)\n", err, ob_sdk_errmsg(rv));
        return rv == OB_E_BUSY ? 3 : 2;
    }
    fprintf(stderr, "openbcm-asic-temp: unit %d %s\n", unit, err);
    {
        /* Discard the first read: the peak latch is garbage until a read
         * resets it (the SDK does the same dummy read at init). */
        int c0[MAXMON], p0[MAXMON], n0 = 0;
        struct timespec ts = {1, 0};
        (void)ob_sdk_temperatures(unit, c0, p0, MAXMON, &n0);
        nanosleep(&ts, NULL);
    }

    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = on_signal;   /* no SA_RESTART: the sleep below wakes up */
    sigaction(SIGTERM, &sa, NULL);
    sigaction(SIGINT, &sa, NULL);
    sigaction(SIGHUP, &sa, NULL);

    while (!stop_flag) {
        int cur[MAXMON], peak[MAXMON], n = 0, i, hot, w;
        long sum = 0;
        rv = ob_sdk_temperatures(unit, cur, peak, MAXMON, &n);
        if (rv < 0 || !ob_temp_plausible(cur, n)) {
            fprintf(stderr, "openbcm-asic-temp: read failed: %s (%d monitors%s)\n",
                    rv < 0 ? ob_sdk_errmsg(rv) : "ok", n, rv >= 0 && n > 0 ? ", implausible value" : "");
            ob_temp_remove(out, prom);
            if (once || ++fails >= 3) return 4;
        } else if (once) {
            hot = ob_temp_hottest(cur, n, &sum);
            for (i = 0; i < n; i++)
                printf("monitor %d: current %.1f C, peak %.1f C\n", i, cur[i] / 10.0, peak[i] / 10.0);
            printf("max %.1f C, average %.1f C\n", cur[hot] / 10.0, sum / 10.0 / n);
            return 0;
        } else {
            fails = 0;
            hot = ob_temp_hottest(cur, n, &sum);
            if (shutdown_at > 0 && cur[hot] >= shutdown_at * 10) {
                if (++hot_reads >= 2) {
                    fprintf(stderr, "openbcm-asic-temp: CRITICAL: monitor %d at %.1f C (limit %d C, %d reads in a row): "
                                    "powering down the switch core PLL as the SDK's over-temperature handler does; "
                                    "the switch ASIC needs a power cycle\n", hot, cur[hot] / 10.0, shutdown_at, hot_reads);
                    ob_temp_remove(out, prom);
                    rv = ob_sdk_th_overtemp_shutdown(unit);
                    if (rv < 0)
                        fprintf(stderr, "openbcm-asic-temp: CRITICAL: core PLL power-down failed: %s\n", ob_sdk_errmsg(rv));
                    return 5;
                }
                fprintf(stderr, "openbcm-asic-temp: monitor %d at %.1f C, at or above the %d C limit "
                                "(core PLL power-down on the next such read)\n", hot, cur[hot] / 10.0, shutdown_at);
            } else {
                hot_reads = 0;
            }
            w = ob_temp_write(out, prom, unit, cur, peak, n);
            if (w & OB_TEMP_W_OUT) fprintf(stderr, "openbcm-asic-temp: cannot write %s: %s\n", out, strerror(errno));
            if (w & OB_TEMP_W_PROM) fprintf(stderr, "openbcm-asic-temp: cannot write %s: %s\n", prom, strerror(errno));
            if (first) {
                fprintf(stderr, "openbcm-asic-temp: %d monitors, max %.1f C, average %.1f C; every %d s to %s; "
                                "core PLL power-down at %d C%s\n",
                        n, cur[hot] / 10.0, sum / 10.0 / n, interval, out, shutdown_at, shutdown_at ? "" : " (off)");
                first = 0;
            }
        }
        {
            struct timespec ts = {interval, 0};
            while (!stop_flag && nanosleep(&ts, &ts) != 0 && errno == EINTR) {}
        }
    }
    ob_temp_remove(out, prom);
    fprintf(stderr, "openbcm-asic-temp: stopped\n");
    return 0;
}
