/* SPDX-License-Identifier: Apache-2.0
 * openbcm-asic-temp: read the switch ASIC's die temperature monitors through
 * the Broadcom SDK and publish them for fan control and monitoring.
 *
 * Attaches only (sdk_glue.c ob_sdk_attach): no chip reset, no "init all",
 * ports and forwarding untouched (on a never-initialised chip it programs
 * the S-bus ring map and PVTMON trim first, as the SDK's chip reset does). It holds the per-unit SDK lock while it
 * runs, so bcm.user / syncd cannot start next to it (they would share the
 * DMA pool and the S-channel): stop it first. Note that only "init all" arms
 * the chip's own over-temperature shutdown (SDK default 125 C); this reader
 * does not.
 *
 *   openbcm-asic-temp --once                 print the monitors, exit
 *   openbcm-asic-temp -o FILE [-p PROM] [-i SECONDS] [-u UNIT]
 *     FILE: hottest current monitor in C ("41.8\n"), replaced atomically each
 *     read; PROM: Prometheus textfile (switch_asic_*). Both are removed on
 *     exit and after a failed read, so a consumer sees "no reading" rather
 *     than an old one. Exits non-zero after 3 failed reads in a row.
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

#define MAXMON 16

static volatile sig_atomic_t stop_flag;
static void on_signal(int s) { (void)s; stop_flag = 1; }

static int write_atomic(const char* path, const char* text)
{
    char tmp[4096];
    FILE* f;
    if (snprintf(tmp, sizeof(tmp), "%s.tmp", path) >= (int)sizeof(tmp)) return -1;
    f = fopen(tmp, "w");
    if (!f) return -1;
    if (fputs(text, f) == EOF) { fclose(f); unlink(tmp); return -1; }
    if (fclose(f) != 0 || rename(tmp, path) != 0) { unlink(tmp); return -1; }
    return 0;
}

static void drop(const char* out, const char* prom)
{
    if (out) unlink(out);
    if (prom) unlink(prom);
}

static void usage(const char* me)
{
    fprintf(stderr, "usage: %s [--unit N] (--once | --out FILE [--prom FILE] [--interval SECONDS])\n", me);
}

int main(int argc, char** argv)
{
    static const struct option opts[] = {
        {"unit", required_argument, 0, 'u'}, {"interval", required_argument, 0, 'i'},
        {"out", required_argument, 0, 'o'}, {"prom", required_argument, 0, 'p'},
        {"once", no_argument, 0, '1'}, {"help", no_argument, 0, 'h'}, {0, 0, 0, 0}};
    int unit = 0, interval = 5, once = 0, c, rv, fails = 0, first = 1;
    const char *out = NULL, *prom = NULL;
    char err[256] = "";
    struct sigaction sa;

    while ((c = getopt_long(argc, argv, "u:i:o:p:1h", opts, NULL)) != -1) {
        switch (c) {
        case 'u': unit = atoi(optarg); break;
        case 'i': interval = atoi(optarg); break;
        case 'o': out = optarg; break;
        case 'p': prom = optarg; break;
        case '1': once = 1; break;
        default: usage(argv[0]); return c == 'h' ? 0 : 1;
        }
    }
    if ((!once && !out) || interval < 1 || interval > 3600) { usage(argv[0]); return 1; }
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
        int cur[MAXMON], peak[MAXMON], n = 0, i, hot = -10000, sum = 0, bad = 0;
        rv = ob_sdk_temperatures(unit, cur, peak, MAXMON, &n);
        for (i = 0; i < n; i++) {
            if (cur[i] < -400 || cur[i] > 1500) bad = 1;   /* outside -40..150 C */
            if (cur[i] > hot) hot = cur[i];
            sum += cur[i];
        }
        if (rv < 0 || n <= 0 || bad) {
            fprintf(stderr, "openbcm-asic-temp: read failed: %s (%d monitors%s)\n",
                    rv < 0 ? ob_sdk_errmsg(rv) : "ok", n, bad ? ", implausible value" : "");
            drop(out, prom);
            if (once || ++fails >= 3) return 4;
        } else if (once) {
            for (i = 0; i < n; i++)
                printf("monitor %d: current %.1f C, peak %.1f C\n", i, cur[i] / 10.0, peak[i] / 10.0);
            printf("max %.1f C, average %.1f C\n", hot / 10.0, sum / 10.0 / n);
            return 0;
        } else {
            char line[64], text[8192];
            size_t len = 0;
            fails = 0;
            snprintf(line, sizeof(line), "%.1f\n", hot / 10.0);
            if (write_atomic(out, line) != 0)
                fprintf(stderr, "openbcm-asic-temp: cannot write %s: %s\n", out, strerror(errno));
            if (prom) {
                len += snprintf(text + len, sizeof(text) - len,
                    "# HELP switch_asic_temperature_celsius ASIC die temperature monitor, current\n"
                    "# TYPE switch_asic_temperature_celsius gauge\n");
                for (i = 0; i < n; i++)
                    len += snprintf(text + len, sizeof(text) - len,
                        "switch_asic_temperature_celsius{unit=\"%d\",sensor=\"pvtmon%d\"} %.1f\n", unit, i, cur[i] / 10.0);
                len += snprintf(text + len, sizeof(text) - len,
                    "# HELP switch_asic_temperature_peak_celsius ASIC die temperature monitor, highest since the previous read\n"
                    "# TYPE switch_asic_temperature_peak_celsius gauge\n");
                for (i = 0; i < n; i++)
                    len += snprintf(text + len, sizeof(text) - len,
                        "switch_asic_temperature_peak_celsius{unit=\"%d\",sensor=\"pvtmon%d\"} %.1f\n", unit, i, peak[i] / 10.0);
                len += snprintf(text + len, sizeof(text) - len,
                    "# HELP switch_asic_temperature_max_celsius hottest current ASIC monitor (what fan control reads)\n"
                    "# TYPE switch_asic_temperature_max_celsius gauge\n"
                    "switch_asic_temperature_max_celsius{unit=\"%d\"} %.1f\n"
                    "# HELP switch_asic_temperature_read_timestamp_seconds time of the last good read\n"
                    "# TYPE switch_asic_temperature_read_timestamp_seconds gauge\n"
                    "switch_asic_temperature_read_timestamp_seconds{unit=\"%d\"} %ld\n",
                    unit, hot / 10.0, unit, (long)time(NULL));
                if (len >= sizeof(text) || write_atomic(prom, text) != 0)
                    fprintf(stderr, "openbcm-asic-temp: cannot write %s\n", prom);
            }
            if (first) {
                fprintf(stderr, "openbcm-asic-temp: %d monitors, max %.1f C, average %.1f C; every %d s to %s\n",
                        n, hot / 10.0, sum / 10.0 / n, interval, out);
                first = 0;
            }
        }
        {
            struct timespec ts = {interval, 0};
            while (!stop_flag && nanosleep(&ts, &ts) != 0 && errno == EINTR) {}
        }
    }
    drop(out, prom);
    fprintf(stderr, "openbcm-asic-temp: stopped\n");
    return 0;
}
