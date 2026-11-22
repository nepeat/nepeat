/* SPDX-License-Identifier: Apache-2.0
 * openbcm-mdio-script: run MDIO scripts (mdio_script.h) through the switch
 * chip's external MDIO master, for PHYs the SDK has no driver for. Run once
 * per boot, before syncd (NixOS: hardware.openbcm.mdioScript).
 *
 *   openbcm-mdio-script --check SCRIPT...                         parse only (any build)
 *   openbcm-mdio-script --config CONFIG.BCM [options] SCRIPT...   run
 *     --unit N
 *     --status-file FILE.json   machine-readable result, rewritten while it runs and at the end,
 *                               on success and on every failure; FILE.txt next to it is the same
 *                               for people (line 1: the one-line outcome)
 *     --names FILE              names for PHYs and registers in messages and tables (mdio_report.h)
 *     --hint CLASS=TEXT         this board's "what to do" for a failure class (repeatable)
 *     --temp-out FILE [--temp-prom FILE] [--temp-interval SECONDS]
 *                               publish the ASIC die temperature while the script runs, exactly as
 *                               openbcm-asic-temp and syncd do (temp_files.h): nothing else can read
 *                               it while this process owns the SDK. Read between two script
 *                               operations by the thread that runs the script; removed on exit.
 *   openbcm-mdio-script --verify-only (--diag-socket SOCKET | --config CONFIG.BCM) [--names FILE] [--json] SCRIPT...
 *                               read-only: only the scripts' verification sections (the assertions
 *                               at their end), every PHY, as a table or JSON. Through syncd's diag
 *                               socket while syncd owns the chip (root), else with the SDK itself.
 *
 * Every script is parsed completely before the chip is touched. With
 * --config the SDK is brought up with "init soc" + "init misc" only
 * (sdk_glue.h ob_sdk_init_mdio: the MDIO clock and hold delay are
 * programmed and read back, no port is enabled; this resets the switch
 * chip); the process holds the per-unit SDK lock, so it cannot run next to
 * syncd, bcm.user or openbcm-asic-temp. syncd's later "init all" resets the
 * switch chip, not the PHYs.
 * A failure names the script line, the PHY and register, expected and read
 * value and how far the script got, and is classified (mdio_report.h
 * MDIO_CLASS_*); after a failure on the chip the verification section is
 * read once more (reads only) to tell a PHY that left the sequence from one
 * that no longer answers.
 * Exit: 0 done, 1 usage, 2 a script failed on the chip (poll, I/O,
 * verification), 3 SDK lock busy, 4 SDK init failed, 5 script missing or
 * malformed, 7 stopped by a signal.
 */
#include <errno.h>
#include <getopt.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include "mdio_report.h"
#include "mdio_script.h"
#ifdef OB_HAVE_SDK
#include "sdk_glue.h"
#include "temp_files.h"
#endif

#define ME "openbcm-mdio-script"
#define MAX_HINTS 16

static volatile sig_atomic_t g_stop;
static void on_signal(int s) { (void)s; g_stop = 1; }

static mdio_status_t g_st;
static mdio_report_t g_report;
static mdio_names_t g_names;
static const char* g_status_file;
static const char* g_diag_socket;
static char g_io_err[300];
static int g_unit;
static struct { const char* cls; const char* text; } g_hint[MAX_HINTS];
static int g_nhint;
static double g_t0;

static double now(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec / 1e9;
}

static void on_verify(void* c, const mdio_script_check_t* ck)
{
    (void)c;
    mdio_report_add(&g_report, ck);
}

static void status_flush(void)
{
    g_st.seconds = now() - g_t0;
    if (g_status_file && mdio_status_write(g_status_file, &g_st) != 0)
        fprintf(stderr, ME ": cannot write %s: %s\n", g_status_file, strerror(errno));
}

/* The run is over: fill in the summary, write the files, tell systemd and the log. */
static void finish(const char* state, const char* cls)
{
    int i;
    char line[1200];
    g_st.state = state;
    g_st.fail_class = cls ? cls : "";
    g_st.finished = (long long)time(NULL);
    g_st.seconds = now() - g_t0;
    for (i = 0; cls && i < g_nhint; i++)
        if (!strcmp(g_hint[i].cls, cls)) snprintf(g_st.hint, sizeof(g_st.hint), "%s", g_hint[i].text);
    mdio_status_summarize(&g_st);
    status_flush();
    if (!strcmp(state, "ok")) {
        snprintf(line, sizeof(line), "ok: %s", g_st.summary);
    } else {
        if (g_st.report && g_st.report->nphy && mdio_report_count(g_st.report, MDIO_PHY_OK) != g_st.report->nphy) {
            fprintf(stderr, ME ": PHY registers read at %s:\n", g_st.report_when);
            mdio_report_table(stderr, g_st.report, g_st.names);
        }
        snprintf(line, sizeof(line), "FAILED (%s): %s", cls && *cls ? cls : state, g_st.summary);
    }
    fprintf(stderr, ME ": %s\n", line);
    if (g_st.hint[0]) fprintf(stderr, ME ": what to do: %s\n", g_st.hint);
    mdio_sd_status(line);
}

static int diag_read(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned* val)
{
    (void)c;
    return mdio_diag_c45_read(g_diag_socket, phy, dev, reg, val, g_io_err, sizeof(g_io_err));
}
static int no_write(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    (void)c; (void)phy; (void)dev; (void)reg; (void)val;
    return -1;   /* verification sections have no writes */
}

#ifdef OB_HAVE_SDK
static void log_line(void* c, const char* msg)
{
    (void)c;
    fprintf(stderr, ME ": %s\n", msg);
}

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

static const char *g_temp_out, *g_temp_prom;
static int g_temp_interval = 5;
static double g_next_temp, g_next_status;

static void publish_temp(void)
{
    int cur[OB_TEMP_MAX_MON], peak[OB_TEMP_MAX_MON], n = 0, rv, w;
    double t = now(), ms;
    rv = ob_sdk_temperatures(g_unit, cur, peak, OB_TEMP_MAX_MON, &n);
    ms = (now() - t) * 1000.0;
    if (ms > g_st.temp_max_ms) g_st.temp_max_ms = ms;
    if (rv < 0 || !ob_temp_plausible(cur, n)) {
        if (!g_st.temp_failed++)
            fprintf(stderr, ME ": ASIC temperature read failed: %s (%d monitors%s); files removed\n",
                    rv < 0 ? ob_sdk_errmsg(rv) : "ok", n, rv >= 0 && n > 0 ? ", implausible value" : "");
        ob_temp_remove(g_temp_out, g_temp_prom);
        return;
    }
    w = ob_temp_write(g_temp_out, g_temp_prom, g_unit, cur, peak, n);
    if (w && !g_st.temp_reads)
        fprintf(stderr, ME ": cannot write %s: %s\n", (w & OB_TEMP_W_OUT) ? g_temp_out : g_temp_prom, strerror(errno));
    g_st.temp_last_c = cur[ob_temp_hottest(cur, n, NULL)] / 10.0;
    if (!g_st.temp_reads++)
        fprintf(stderr, ME ": ASIC temperature: %d monitors, max %.1f C; every %d s to %s while the script runs\n", n,
                g_st.temp_last_c, g_temp_interval, g_temp_out);
}

/* Between two script operations, in the thread that runs the script. */
static int tick(void* c)
{
    double t;
    (void)c;
    if (g_stop) return 1;
    t = now();
    if (g_temp_out && t >= g_next_temp) {
        publish_temp();
        g_next_temp = t + g_temp_interval;
    }
    if (t >= g_next_status) {
        mdio_status_summarize(&g_st);
        status_flush();
        g_st.summary[0] = 0;   /* rebuilt each time while running */
        g_next_status = t + 5.0;
    }
    return 0;
}
#endif

static void usage(void)
{
    fprintf(stderr,
            "usage: " ME " --check SCRIPT...\n"
            "       " ME " --config CONFIG.BCM [--unit N] [--status-file FILE.json] [--names FILE]\n"
            "              [--hint CLASS=TEXT]... [--temp-out FILE [--temp-prom FILE] [--temp-interval S]] SCRIPT...\n"
            "       " ME " --verify-only (--diag-socket SOCKET | --config CONFIG.BCM) [--names FILE] [--json] SCRIPT...\n");
}

int main(int argc, char** argv)
{
    static const struct option opts[] = {
        {"check", no_argument, 0, 'c'}, {"config", required_argument, 0, 'f'},
        {"unit", required_argument, 0, 'u'}, {"verify-only", no_argument, 0, 'v'},
        {"diag-socket", required_argument, 0, 'd'}, {"names", required_argument, 0, 'n'},
        {"json", no_argument, 0, 'j'}, {"status-file", required_argument, 0, 's'},
        {"hint", required_argument, 0, 'H'}, {"temp-out", required_argument, 0, 'o'},
        {"temp-prom", required_argument, 0, 'p'}, {"temp-interval", required_argument, 0, 'i'},
        {"help", no_argument, 0, 'h'}, {0, 0, 0, 0}};
    int c, i, check = 0, verify = 0, json = 0, rv, temp_interval = 5, nver = 0;
    const char *config = NULL, *names = NULL, *temp_out = NULL, *temp_prom = NULL;
    char err[600] = "";
    struct sigaction sa;

    while ((c = getopt_long(argc, argv, "cf:u:vd:n:js:H:o:p:i:h", opts, NULL)) != -1) {
        switch (c) {
        case 'c': check = 1; break;
        case 'f': config = optarg; break;
        case 'u': g_unit = atoi(optarg); break;
        case 'v': verify = 1; break;
        case 'd': g_diag_socket = optarg; break;
        case 'n': names = optarg; break;
        case 'j': json = 1; break;
        case 's': g_status_file = optarg; break;
        case 'H': {
            char* eq = strchr(optarg, '=');
            if (!eq || g_nhint == MAX_HINTS) {
                usage();
                return 1;
            }
            *eq = 0;
            g_hint[g_nhint].cls = optarg;
            g_hint[g_nhint++].text = eq + 1;
            break;
        }
        case 'o': temp_out = optarg; break;
        case 'p': temp_prom = optarg; break;
        case 'i': temp_interval = atoi(optarg); break;
        default: usage(); return c == 'h' ? 0 : 1;
        }
    }
    if (optind >= argc || argc - optind > MDIO_STATUS_MAX_SCRIPTS || temp_interval < 1 || temp_interval > 3600 ||
        (check && verify) || (!check && !config && !(verify && g_diag_socket)) || (g_diag_socket && !verify) ||
        (temp_prom && !temp_out)) {
        usage();
        return 1;
    }
    if (verify) g_status_file = NULL;   /* a look at the PHYs is not a run: the last run's record stays */
    g_t0 = now();
    g_st.state = "running";
    g_st.mode = verify ? "verify" : "run";
    g_st.started = (long long)time(NULL);
    g_st.pid = (int)getpid();
    g_st.fail_class = "";
    g_st.nscripts = argc - optind;
    for (i = 0; i < g_st.nscripts; i++) g_st.script[i].path = argv[optind + i];
    if (names) {
        if (mdio_names_load(names, &g_names, err, sizeof(err)) != 0) fprintf(stderr, ME ": %s (names ignored)\n", err);
        else g_st.names = &g_names;
    }

    /* 1. Every script must parse before anything is touched. */
    for (i = 0; i < g_st.nscripts; i++) {
        mdio_status_script_t* s = &g_st.script[i];
        rv = mdio_script_run(s->path, NULL, &s->parsed, err, sizeof(err));
        if (rv != MDIO_SCRIPT_OK) {
            g_st.fail_script = s->path;
            snprintf(g_st.summary, sizeof(g_st.summary), "%s%s", err,
                     check || verify ? "" : "; nothing was written to any PHY");
            if (check) {
                fprintf(stderr, ME ": %s\n", g_st.summary);
                return 5;
            }
            finish("failed", mdio_classify_run(rv, NULL, NULL));
            return 5;
        }
        if (s->parsed.verify_line) nver++;
        if (check || !verify)
            fprintf(stderr, ME ": %s: %lu writes, %lu reads (%lu polls), %lu delays (%.1f s), %lu steps; verification: %lu "
                            "reads from line %lu\n", s->path, s->parsed.writes, s->parsed.reads, s->parsed.polls,
                    s->parsed.delays, (double)s->parsed.delay_us / 1e6, s->parsed.labels, s->parsed.verify_checks,
                    s->parsed.verify_line);
    }
    if (check) return 0;
    if (verify && !nver) {
        fprintf(stderr, ME ": no script has a verification section (assertions at its end): nothing to read\n");
        return 5;
    }

    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = on_signal;
    sigaction(SIGTERM, &sa, NULL);
    sigaction(SIGINT, &sa, NULL);

    /* 2. Verification only, through syncd: no SDK in this process. */
    if (verify && g_diag_socket) {
        mdio_script_io_t io = {diag_read, no_write, NULL, NULL, NULL, NULL, on_verify};
        mdio_script_opts_t o = {0, 1, NULL};
        snprintf(g_st.sdk, sizeof(g_st.sdk), "through syncd's diag socket %s", g_diag_socket);
        for (i = 0; i < g_st.nscripts; i++) {
            mdio_status_script_t* s = &g_st.script[i];
            if (!s->parsed.verify_line) continue;
            o.verify_line = s->parsed.verify_line;
            s->started = 1;
            s->result = mdio_script_run_opts(s->path, &io, &o, &s->run, err, sizeof(err));
        }
        goto verify_report;
    }

#ifndef OB_HAVE_SDK
    (void)temp_out;
    fprintf(stderr, ME ": built without the SDK: --check and --verify-only --diag-socket only\n");
    return 1;
#else
    setvbuf(stdout, NULL, _IOLBF, 0);
    if (!verify) {
        status_flush();
        mdio_sd_status("bringing up the SDK (init soc, init misc)");
    }
    rv = ob_sdk_init_mdio(g_unit, config, err, sizeof(err));
    if (rv < 0) {
        const char* cls = rv == OB_E_BUSY ? MDIO_CLASS_SDK_BUSY : rv == OB_E_CONFIG ? MDIO_CLASS_SDK_CONFIG
                        : rv == OB_E_MDIO_CLOCK ? MDIO_CLASS_MDIO_CLOCK : MDIO_CLASS_SDK_INIT;
        if (rv == OB_E_BUSY) {
            char lock[64], comm[32];
            int pid = 0;
            snprintf(lock, sizeof(lock), "/run/openbcm-unit%d.lock", g_unit);
            if (mdio_lock_holder(lock, &pid, comm, sizeof(comm)))
                snprintf(g_st.summary, sizeof(g_st.summary), "the SDK lock %s is held by pid %d (%s): that process owns "
                         "the switch chip; nothing was written to any PHY", lock, pid, comm[0] ? comm : "?");
            else
                snprintf(g_st.summary, sizeof(g_st.summary), "%s; nothing was written to any PHY", err);
        } else {
            snprintf(g_st.summary, sizeof(g_st.summary), "%s (%s); nothing was written to any PHY", err, ob_sdk_errmsg(rv));
        }
        if (verify) {
            fprintf(stderr, ME ": %s\n", g_st.summary);
            if (rv == OB_E_BUSY)
                fprintf(stderr, ME ": while syncd owns the chip, read through it: --verify-only --diag-socket SOCKET\n");
        } else {
            finish("failed", cls);
        }
        return rv == OB_E_BUSY ? 3 : 4;
    }
    snprintf(g_st.sdk, sizeof(g_st.sdk), "%s", err);
    fprintf(stderr, ME ": %s\n", err);

    if (verify) {
        mdio_script_io_t io = {sdk_read, no_write, NULL, NULL, NULL, NULL, on_verify};
        mdio_script_opts_t o = {0, 1, NULL};
        for (i = 0; i < g_st.nscripts; i++) {
            mdio_status_script_t* s = &g_st.script[i];
            if (!s->parsed.verify_line) continue;
            o.verify_line = s->parsed.verify_line;
            s->started = 1;
            s->result = mdio_script_run_opts(s->path, &io, &o, &s->run, err, sizeof(err));
        }
        goto verify_report;
    }

    /* 3. The run. */
    {
        mdio_script_io_t io = {sdk_read, sdk_write, NULL, log_line, NULL, tick, on_verify};
        if (temp_out) {
            int c0[OB_TEMP_MAX_MON], p0[OB_TEMP_MAX_MON], n0 = 0;
            g_temp_out = temp_out;
            g_temp_prom = temp_prom;
            g_temp_interval = temp_interval;
            g_st.temp_enabled = 1;
            /* The first read after a reset returns a garbage peak (the SDK discards one too). */
            (void)ob_sdk_temperatures(g_unit, c0, p0, OB_TEMP_MAX_MON, &n0);
        }
        mdio_sd_status("running the script");
        for (i = 0; i < g_st.nscripts; i++) {
            mdio_status_script_t* s = &g_st.script[i];
            mdio_script_opts_t o = {s->parsed.verify_line, 0, &g_st.fail};
            double t0 = now();
            s->started = 1;
            rv = mdio_script_run_opts(s->path, &io, &o, &s->run, err, sizeof(err));
            s->result = rv;
            s->seconds = now() - t0;
            fprintf(stderr, ME ": %s: %s in %.1f s: %lu of %lu operations, %lu writes, %lu reads (%lu polls, %lu "
                            "re-reads), %lu expected values differed\n", s->path, rv == MDIO_SCRIPT_OK ? "done" : "FAILED",
                    s->seconds, s->run.ops, s->parsed.ops, s->run.writes, s->run.reads, s->run.polls,
                    s->run.poll_rereads, s->run.differs);
            if (rv == MDIO_SCRIPT_OK) continue;
            fprintf(stderr, ME ": %s\n", err);
            g_st.fail_script = s->path;
            if (rv == MDIO_SCRIPT_E_POLL && s->parsed.verify_line && !g_stop) {
                /* Post-mortem, reads only: does the PHY still answer? */
                mdio_script_io_t pio = {sdk_read, no_write, NULL, NULL, NULL, NULL, on_verify};
                mdio_script_opts_t po = {s->parsed.verify_line, 1, NULL};
                mdio_script_stats_t pst;
                char perr[300];
                mdio_report_reset(&g_report);
                (void)mdio_script_run_opts(s->path, &pio, &po, &pst, perr, sizeof(perr));
                g_st.report_when = "after the failure (reads only)";
            } else {
                g_st.report_when = "the end of the script";
            }
            if (g_report.nphy) {
                mdio_report_classify(&g_report);
                g_st.report = &g_report;
            }
            if (g_temp_out) ob_temp_remove(g_temp_out, g_temp_prom);
            finish(rv == MDIO_SCRIPT_E_ABORT ? "interrupted" : "failed", mdio_classify_run(rv, &g_st.fail, g_st.report));
            return rv == MDIO_SCRIPT_E_ABORT ? 7 : 2;
        }
        if (g_temp_out) ob_temp_remove(g_temp_out, g_temp_prom);
        if (g_report.nphy) {
            mdio_report_classify(&g_report);
            g_st.report = &g_report;
            g_st.report_when = "the end of the script";
        }
        finish("ok", NULL);
        return 0;
    }
#endif

verify_report:
    mdio_report_classify(&g_report);
    g_st.report = &g_report;
    g_st.report_when = "now";
    g_st.finished = (long long)time(NULL);
    g_st.seconds = now() - g_t0;
    {
        int bad = g_report.nphy - mdio_report_count(&g_report, MDIO_PHY_OK), io_bad = mdio_report_count(&g_report, MDIO_PHY_IO);
        g_st.state = bad ? "failed" : "ok";
        if (bad) {
            for (i = 0; i < g_st.nscripts; i++)
                if (g_st.script[i].started && g_st.script[i].result != MDIO_SCRIPT_OK) g_st.fail_script = g_st.script[i].path;
            g_st.fail_class = mdio_classify_run(MDIO_SCRIPT_E_VERIFY, NULL, &g_report);
            for (i = 0; i < g_nhint; i++)
                if (!strcmp(g_hint[i].cls, g_st.fail_class)) snprintf(g_st.hint, sizeof(g_st.hint), "%s", g_hint[i].text);
        }
        mdio_status_summarize(&g_st);
        if (json) {
            mdio_status_json(stdout, &g_st);
        } else {
            mdio_report_table(stdout, &g_report, g_st.names);
            if (bad) printf("%d of %d PHYs are not as the script expects (%s): %s\n", bad, g_report.nphy, g_st.fail_class,
                            g_st.summary);
            else printf("%d of %d PHYs as the script expects\n", g_report.nphy, g_report.nphy);
            if (bad && g_st.hint[0]) printf("what to do: %s\n", g_st.hint);
        }
        if (io_bad && g_io_err[0]) fprintf(stderr, ME ": %s\n", g_io_err);
        return bad ? 2 : 0;
    }
}
