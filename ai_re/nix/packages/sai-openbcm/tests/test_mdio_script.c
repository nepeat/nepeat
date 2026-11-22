/* SPDX-License-Identifier: Apache-2.0
 * Unit test of the MDIO script interpreter (src/mdio_script.c) on a fake
 * bus, of the reporting around it (src/mdio_report.c: per-PHY verification
 * results, failure classes, status files, SDK lock holder, reads through a
 * fake syncd diag socket) and, when the tool's path is given as argv[1], of
 * openbcm-mdio-script itself for everything that needs no SDK. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

#include "mdio_report.h"
#include "mdio_script.h"

static int failures;
#define CHECK(c)                                                              \
    do {                                                                      \
        if (!(c)) {                                                           \
            fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #c);      \
            failures++;                                                       \
        }                                                                     \
    } while (0)

/* Fake bus: a log of accesses, a small register file, one register that
 * becomes ready after N reads, one that fails. */
static char trace[4096];
static unsigned regs[0x10000];
static int ready_after, io_calls;
static unsigned long slept;

static int f_read(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned* val)
{
    char b[64];
    (void)c;
    io_calls++;
    if (reg == 0xdead) return -1;
    if (reg == 0x8221) *val = ready_after-- > 0 ? 0 : 0x0303;
    else *val = regs[reg];
    snprintf(b, sizeof(b), "R%x.%x.%x ", phy, dev, reg);
    strncat(trace, b, sizeof(trace) - strlen(trace) - 1);
    return 0;
}
static int f_write(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    char b[64];
    (void)c;
    io_calls++;
    regs[reg] = val;
    snprintf(b, sizeof(b), "W%x.%x.%x=%x ", phy, dev, reg, val);
    strncat(trace, b, sizeof(trace) - strlen(trace) - 1);
    return 0;
}
static void f_sleep(void* c, unsigned long us) { (void)c; slept += us; }
static char last_log[4096];
static void f_log(void* c, const char* m)
{
    (void)c;
    strncat(last_log, m, sizeof(last_log) - strlen(last_log) - 2);
    strcat(last_log, "\n");
}
static const mdio_script_io_t io = {f_read, f_write, f_sleep, f_log, NULL, NULL, NULL};

static const char* put(const char* text)
{
    static char path[64];
    FILE* f;
    snprintf(path, sizeof(path), "/tmp/test_mdio_script.%d", (int)getpid());
    if (!text) return path;   /* just the name */
    f = fopen(path, "w");
    fputs(text, f);
    fclose(f);
    return path;
}

static int run(const char* text, int with_io, mdio_script_stats_t* st, char* err)
{
    trace[0] = last_log[0] = 0;
    io_calls = 0;
    slept = 0;
    return mdio_script_run(put(text), with_io ? &io : NULL, st, err, 256);
}

/* ---- a bus of several PHYs for the verification / report tests ----
 * phy 0x0 and 0x1 are healthy; 0x24 is "hung": one constant on every
 * register; 0x25 answers but is not initialised (1.8215 = 0001); 0x26 makes
 * the bus master fail. Writes are counted. Register 1.8221 of phy 0x1 never
 * becomes ready when stuck is set. */
static unsigned bus_writes, bus_reads, bus_ticks, stuck, abort_at_tick, hang_from_now;
static int b_read(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned* val)
{
    (void)c; (void)dev;
    bus_reads++;
    if (phy == 0x26) return -1;
    if (phy == 0x24 || (hang_from_now && phy == 0x1)) *val = 0x1900;
    else if (reg == 0x8b00) *val = 0x2391;
    else if (reg == 0x8b01) *val = 0x80a0;
    else if (reg == 0x8215) *val = phy == 0x25 ? 0x0001 : 0xd00b;
    else if (reg == 0x8251) *val = 0x002d;
    else if (reg == 0x8221) *val = stuck ? 0 : 0x0303;
    else *val = 0;
    return 0;
}
static int b_write(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    (void)c; (void)phy; (void)dev; (void)reg; (void)val;
    bus_writes++;
    return 0;
}
static int b_tick(void* c)
{
    (void)c;
    bus_ticks++;
    return abort_at_tick && bus_ticks >= abort_at_tick;
}
static mdio_report_t rep;
static void b_verify(void* c, const mdio_script_check_t* ck)
{
    (void)c;
    mdio_report_add(&rep, ck);
}
static const mdio_script_io_t bus = {b_read, b_write, f_sleep, NULL, NULL, b_tick, b_verify};

#define VERIFY(phy) "L verify phy 0x" #phy "\nP " #phy " 1 8b00 2391 ffff 1 0\nP " #phy " 1 8b01 80a0 ffff 1 0\n" \
                    "P " #phy " 1 8215 d00b ffff 1 0\nP " #phy " 1 8251 000d 000d 1 0\n"
#define BODY "mdio-script 1\nL init\nW 0 1 8201 7e\nW 1 1 8201 7e\nD 10\nP 1 1 8221 0303 ffff 5 10\nW 1 1 1 1\n"

static char* slurp(const char* path)
{
    static char b[65536];
    FILE* f = fopen(path, "r");
    size_t n = f ? fread(b, 1, sizeof(b) - 1, f) : 0;
    if (f) fclose(f);
    b[n] = 0;
    return b;
}

static int has(const char* text, const char* needle)
{
    if (strstr(text, needle)) return 1;
    fprintf(stderr, "  missing: %s\n  in: %s\n", needle, text);
    return 0;
}

/* Fake syncd diag socket: answers "phy raw c45 <phy> <dev> <reg>" from b_read. */
static char sock_path[100];
static void* diag_server(void* arg)
{
    int lfd = *(int*)arg;
    for (;;) {
        char cmd[200], out[200];
        unsigned phy, dev, reg, v = 0;
        ssize_t n;
        int fd = accept(lfd, NULL, NULL);
        if (fd < 0) return NULL;
        n = read(fd, cmd, sizeof(cmd) - 1);
        cmd[n > 0 ? n : 0] = 0;
        if (sscanf(cmd, "phy raw c45 0x%x 0x%x 0x%x", &phy, &dev, &reg) != 3)
            snprintf(out, sizeof(out), "openbcm-diag: refused (would end or re-init the SDK, or chains commands): %.60s", cmd);
        else if (b_read(NULL, phy, dev, reg, &v) != 0) snprintf(out, sizeof(out), "openbcm-diag: command failed (-1)\n");
        else snprintf(out, sizeof(out), "\t0x%04x: 0x%04x\n", reg, v);
        (void)!write(fd, out, strlen(out));
        close(fd);
    }
}

/* Run the tool; stdout+stderr to a file whose content is returned. */
static int tool(const char* exe, char** out, const char* a1, const char* a2, const char* a3, const char* a4,
                const char* a5, const char* a6, const char* a7)
{
    static char path[64];
    int status = -1;
    pid_t pid;
    snprintf(path, sizeof(path), "/tmp/test_mdio_script.out.%d", (int)getpid());
    pid = fork();
    if (pid == 0) {
        int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600);
        dup2(fd, 1);
        dup2(fd, 2);
        unsetenv("NOTIFY_SOCKET");
        execl(exe, exe, a1, a2, a3, a4, a5, a6, a7, (char*)NULL);
        _exit(127);
    }
    waitpid(pid, &status, 0);
    *out = slurp(path);
    unlink(path);
    return WIFEXITED(status) ? WEXITSTATUS(status) : -1;
}

static void test_verification(void)
{
    mdio_script_stats_t parsed, st;
    mdio_script_fail_t fail;
    mdio_script_opts_t o = {0, 0, &fail};
    char err[400];
    const char* p;

    /* Parse pass: the verification section is the assertions after the last other operation. */
    p = put(BODY VERIFY(0) "# note\n" VERIFY(1));
    CHECK(mdio_script_run(p, NULL, &parsed, err, sizeof(err)) == MDIO_SCRIPT_OK);
    CHECK(parsed.verify_line == 8 && parsed.verify_checks == 8 && parsed.ops == 13);
    /* a single-try assertion in the middle is not part of it; no assertions at the end: none */
    CHECK(mdio_script_run(put("mdio-script 1\nP 0 1 8b00 2391 ffff 1 0\nW 0 1 1 1\nL x\n"), NULL, &parsed, err,
                          sizeof(err)) == MDIO_SCRIPT_OK && parsed.verify_line == 0 && parsed.verify_checks == 0);
    CHECK(mdio_script_run(put("mdio-script 1\nW 0 1 1 1\nP 0 1 8b00 2391 ffff 2 0\nP 0 1 8b00 2391 ffff 1 0\n"), NULL,
                          &parsed, err, sizeof(err)) == MDIO_SCRIPT_OK && parsed.verify_line == 4);

    /* All good: every check is reported, the script succeeds. */
    p = put(BODY VERIFY(0) VERIFY(1));
    mdio_script_run(p, NULL, &parsed, err, sizeof(err));
    o.verify_line = parsed.verify_line;
    mdio_report_reset(&rep);
    bus_writes = bus_reads = bus_ticks = 0;
    CHECK(mdio_script_run_opts(p, &bus, &o, &st, err, sizeof(err)) == MDIO_SCRIPT_OK);
    CHECK(st.verify_checks == 8 && st.verify_failed == 0 && st.ops == parsed.ops && bus_writes == 3);
    CHECK(bus_ticks == 13);   /* once per operation */
    mdio_report_classify(&rep);
    CHECK(rep.nphy == 2 && mdio_report_count(&rep, MDIO_PHY_OK) == 2 && rep.phy[1].n == 4 && fail.line == 0);

    /* Mismatches in the section do not stop it: all four PHYs are read, then it fails. */
    p = put(BODY VERIFY(0) VERIFY(24) VERIFY(25) VERIFY(26));
    mdio_script_run(p, NULL, &parsed, err, sizeof(err));
    o.verify_line = parsed.verify_line;
    mdio_report_reset(&rep);
    CHECK(mdio_script_run_opts(p, &bus, &o, &st, err, sizeof(err)) == MDIO_SCRIPT_E_VERIFY);
    CHECK(st.verify_checks == 16 && st.verify_failed == 4 + 1 + 4 && st.ops == parsed.ops);
    CHECK(has(err, "9 of 16 verification reads") && has(err, "phy 24 1.8b00 reads 1900, script wants 2391"));
    CHECK(fail.line == 14 && fail.op == 'P' && fail.phy == 0x24 && fail.reg == 0x8b00 && fail.want == 0x2391 &&
          fail.got == 0x1900 && fail.have_value && !strcmp(fail.label, "verify phy 0x24"));
    mdio_report_classify(&rep);
    CHECK(rep.nphy == 4 && rep.phy[0].state == MDIO_PHY_OK && rep.phy[1].state == MDIO_PHY_CONSTANT &&
          rep.phy[1].constant == 0x1900 && rep.phy[2].state == MDIO_PHY_MISMATCH && rep.phy[3].state == MDIO_PHY_IO);
    CHECK(!strcmp(mdio_classify_run(MDIO_SCRIPT_E_VERIFY, &fail, &rep), MDIO_CLASS_PHY_CONSTANT));
    /* without the verify line (the old behaviour) the first mismatch stops the script */
    o.verify_line = 0;
    CHECK(mdio_script_run_opts(p, &bus, &o, &st, err, sizeof(err)) == MDIO_SCRIPT_E_POLL && fail.line == 14 && fail.tries == 1);

    /* verify_only: nothing before the section runs, no write, only its reads. */
    o.verify_line = parsed.verify_line;
    o.verify_only = 1;
    mdio_report_reset(&rep);
    bus_writes = bus_reads = 0;
    slept = 0;
    CHECK(mdio_script_run_opts(p, &bus, &o, &st, err, sizeof(err)) == MDIO_SCRIPT_E_VERIFY);
    CHECK(bus_writes == 0 && bus_reads == 16 && slept == 0 && st.writes == 3 && rep.nphy == 4);
    o.verify_line = 0;
    CHECK(mdio_script_run_opts(p, &bus, &o, &st, err, sizeof(err)) == MDIO_SCRIPT_E_SYNTAX && has(err, "no verification section"));
    o.verify_only = 0;

    /* A poll that never matches: the record says where, what and how often. */
    p = put(BODY VERIFY(0) VERIFY(1));
    mdio_script_run(p, NULL, &parsed, err, sizeof(err));
    o.verify_line = parsed.verify_line;
    stuck = 1;
    bus_writes = 0;
    CHECK(mdio_script_run_opts(p, &bus, &o, &st, err, sizeof(err)) == MDIO_SCRIPT_E_POLL);
    CHECK(fail.line == 6 && fail.op == 'P' && fail.phy == 1 && fail.dev == 1 && fail.reg == 0x8221 && fail.want == 0x0303 &&
          fail.mask == 0xffff && fail.got == 0 && fail.tries == 5 && !strcmp(fail.label, "init"));
    CHECK(st.ops == 3 && bus_writes == 2);   /* W W D done; the poll and what follows are not */
    /* post-mortem: the PHY still answers -> it left the sequence; it reads a constant -> not answering */
    o.verify_only = 1;
    mdio_report_reset(&rep);
    (void)mdio_script_run_opts(p, &bus, &o, &st, err, sizeof(err));
    mdio_report_classify(&rep);
    CHECK(!strcmp(mdio_classify_run(MDIO_SCRIPT_E_POLL, &fail, &rep), MDIO_CLASS_DIVERGED));
    hang_from_now = 1;
    mdio_report_reset(&rep);
    (void)mdio_script_run_opts(p, &bus, &o, &st, err, sizeof(err));
    mdio_report_classify(&rep);
    CHECK(!strcmp(mdio_classify_run(MDIO_SCRIPT_E_POLL, &fail, &rep), MDIO_CLASS_PHY_CONSTANT));
    hang_from_now = stuck = 0;
    o.verify_only = 0;
    CHECK(!strcmp(mdio_classify_run(MDIO_SCRIPT_E_POLL, &fail, NULL), MDIO_CLASS_DIVERGED));
    CHECK(!strcmp(mdio_classify_run(MDIO_SCRIPT_E_FILE, NULL, NULL), MDIO_CLASS_SCRIPT_MISSING));
    CHECK(!strcmp(mdio_classify_run(MDIO_SCRIPT_E_SYNTAX, NULL, NULL), MDIO_CLASS_SCRIPT_MALFORMED));
    CHECK(!strcmp(mdio_classify_run(MDIO_SCRIPT_E_IO, NULL, NULL), MDIO_CLASS_MDIO_IO));
    CHECK(!strcmp(mdio_classify_run(MDIO_SCRIPT_E_ABORT, NULL, NULL), MDIO_CLASS_INTERRUPTED));
    CHECK(*mdio_class_hint(MDIO_CLASS_PHY_CONSTANT) && *mdio_class_hint(MDIO_CLASS_SDK_BUSY) && !*mdio_class_hint("x"));

    /* tick: a stop request ends the script between two operations; long delays are cut into pieces. */
    bus_ticks = bus_writes = 0;
    abort_at_tick = 2;
    CHECK(mdio_script_run_opts(p, &bus, &o, &st, err, sizeof(err)) == MDIO_SCRIPT_E_ABORT);
    CHECK(bus_writes == 1 && st.ops == 1 && has(err, ":4: stopped on request") && fail.line == 4);
    abort_at_tick = 0;
    bus_ticks = 0;
    slept = 0;
    CHECK(mdio_script_run_opts(put("mdio-script 1\nD 1700000\n"), &bus, NULL, &st, err, sizeof(err)) == MDIO_SCRIPT_OK);
    CHECK(slept == 1700000 && bus_ticks == 1 + 3);
    bus_ticks = 0;
    abort_at_tick = 3;
    CHECK(mdio_script_run_opts(put("mdio-script 1\nD 1700000\nW 0 1 1 1\n"), &bus, NULL, &st, err, sizeof(err)) ==
          MDIO_SCRIPT_E_ABORT && st.ops == 0 && has(err, "during a delay"));
    abort_at_tick = 0;
}

static void test_report_and_status(void)
{
    static mdio_status_t s;
    mdio_script_stats_t parsed;
    mdio_script_opts_t o = {0, 0, &s.fail};
    static mdio_names_t names;
    char err[400], dir[64], json[100], txt[100], *text;
    const char* p;
    FILE* f;

    f = fopen(put(""), "w");
    fputs("# names\nphy 0 cage1\nphy 24 cage5\nphy 25 cage6\nreg 1.8b00 id\nreg 1.8215 fw\n", f);
    fclose(f);
    CHECK(mdio_names_load(put(NULL), &names, err, sizeof(err)) == 0 && names.nphy == 3 && names.nreg == 2);
    CHECK(!strcmp(mdio_names_phy(&names, 0x24), "cage5") && !mdio_names_phy(&names, 0x26) &&
          !strcmp(mdio_names_reg(&names, 1, 0x8215), "fw"));
    f = fopen(put(NULL), "w");
    fputs("phy zz cage1\n", f);
    fclose(f);
    CHECK(mdio_names_load(put(NULL), &names, err, sizeof(err)) != 0 && has(err, ":1:"));
    CHECK(mdio_names_load("/nonexistent/names", &names, err, sizeof(err)) != 0);
    f = fopen(put(NULL), "w");
    fputs("phy 0 cage1\nphy 24 cage5\nphy 25 cage6\nreg 1.8b00 id\nreg 1.8215 fw\n", f);
    fclose(f);
    CHECK(mdio_names_load(put(NULL), &names, err, sizeof(err)) == 0);

    /* A run whose verification finds a hung PHY, an uninitialised one and an MDIO error. */
    snprintf(dir, sizeof(dir), "/tmp/test_mdio_status.%d", (int)getpid());
    snprintf(json, sizeof(json), "%s/status.json", dir);
    snprintf(txt, sizeof(txt), "%s/status.txt", dir);
    p = put(BODY VERIFY(0) VERIFY(24) VERIFY(25));
    memset(&s, 0, sizeof(s));
    s.state = "failed";
    s.mode = "run";
    s.started = 1795000000;
    s.finished = 1795000096;
    s.seconds = 95.8;
    s.pid = 4242;
    s.names = &names;
    s.nscripts = 1;
    s.script[0].path = "/var/lib/phy/x \"y\".mdio";
    snprintf(s.sdk, sizeof(s.sdk), "BCM56960_B1: init soc + init misc done");
    mdio_script_run(p, NULL, &s.script[0].parsed, err, sizeof(err));
    parsed = s.script[0].parsed;
    o.verify_line = parsed.verify_line;
    mdio_report_reset(&rep);
    s.script[0].started = 1;
    s.script[0].result = mdio_script_run_opts(p, &bus, &o, &s.script[0].run, err, sizeof(err));
    CHECK(s.script[0].result == MDIO_SCRIPT_E_VERIFY);
    mdio_report_classify(&rep);
    s.report = &rep;
    s.report_when = "the end of the script";
    s.fail_script = s.script[0].path;
    s.fail_class = mdio_classify_run(s.script[0].result, &s.fail, &rep);
    s.temp_enabled = 1;
    s.temp_reads = 19;
    s.temp_last_c = 41.3;
    mdio_status_summarize(&s);
    CHECK(has(s.summary, "x \"y\".mdio line 14 [verify phy 0x24]: verification: 2 of 3 PHYs differ: "
                         "cage5 (phy 0x24) reads 0x1900 on every verified register; "
                         "cage6 (phy 0x25) fw (1.8215) read 0x0001, expected 0xd00b; 17 of 17 operations had run; "
                         "cage5 (phy 0x24) reads 0x1900 on every verified register: not answering or hung"));
    CHECK(has(s.hint, "power-cycle"));
    CHECK(mdio_status_write(json, &s) == 0);
    text = slurp(txt);
    CHECK(strncmp(text, "openbcm-mdio-script: FAILED (phy-not-answering): x \"y\".mdio line 14", 67) == 0);
    CHECK(has(text, "  what to do: a PHY that reads one constant") && has(text, "17 of 17 operations") &&
          has(text, "ASIC temperature published meanwhile: 19 readings, last 41.3 C") &&
          has(text, "name   phy    id      1.8b01  fw      1.8251  state\n") &&
          has(text, "cage1  0x000  2391    80a0    d00b    002d    ok\n") &&
          has(text, "cage5  0x024  1900!   1900!   1900!   1900!   CONSTANT 0x1900: not answering or hung\n") &&
          has(text, "cage6  0x025  2391    80a0    0001!   002d    MISMATCH\n"));
    text = slurp(json);
    CHECK(has(text, "\"state\": \"failed\"") && has(text, "\"class\": \"phy-not-answering\"") &&
          has(text, "\"path\": \"/var/lib/phy/x \\\"y\\\".mdio\"") && has(text, "\"operations_done\": 17") &&
          has(text, "\"line\": 14") && has(text, "\"phy\": \"0x24\", \"phy_name\": \"cage5\"") &&
          has(text, "\"expected\": \"0x2391\", \"mask\": \"0xffff\", \"read\": \"0x1900\"") &&
          has(text, "\"phys_ok\": 1, \"phys_total\": 3") &&
          has(text, "{\"phy\": \"0x25\", \"name\": \"cage6\", \"state\": \"mismatch\"") &&
          has(text, "{\"reg\": \"1.8215\", \"name\": \"fw\", \"read\": \"0x0001\", \"expected\": \"0xd00b\", "
                    "\"mask\": \"0xffff\", \"ok\": false}") &&
          has(text, "\"asic_temperature\": {\"readings\": 19"));

    /* A poll that timed out on a PHY that still answers. */
    p = put(BODY VERIFY(0) VERIFY(1));
    memset(&s, 0, sizeof(s));
    s.state = "failed";
    s.mode = "run";
    s.nscripts = 1;
    s.script[0].path = "/p/init.mdio";
    s.fail_script = s.script[0].path;
    mdio_script_run(p, NULL, &s.script[0].parsed, err, sizeof(err));
    o.verify_line = s.script[0].parsed.verify_line;
    stuck = 1;
    s.script[0].started = 1;
    s.script[0].result = mdio_script_run_opts(p, &bus, &o, &s.script[0].run, err, sizeof(err));
    {
        mdio_script_opts_t po = {o.verify_line, 1, NULL};
        mdio_script_stats_t pst;
        mdio_report_reset(&rep);
        (void)mdio_script_run_opts(p, &bus, &po, &pst, err, sizeof(err));
        mdio_report_classify(&rep);
    }
    stuck = 0;
    s.report = &rep;
    s.report_when = "after the failure (reads only)";
    s.fail_class = mdio_classify_run(s.script[0].result, &s.fail, &rep);
    mdio_status_summarize(&s);
    CHECK(has(s.summary, "init.mdio line 6 [init]: phy 0x1 1.8221 read 0x0000, the script expects 0x0303 (mask 0xffff) "
                         "after 5 reads; 3 of 13 operations had run; the PHY still answers (its registers are not one "
                         "constant): it left the recorded sequence"));
    CHECK(mdio_status_write(json, &s) == 0);
    CHECK(has(slurp(txt), "openbcm-mdio-script: FAILED (sequence-diverged): init.mdio line 6") &&
          has(slurp(txt), "PHY registers read at after the failure (reads only): 2 of 2 PHYs as expected"));
    CHECK(has(slurp(json), "\"reads\": 5") && has(slurp(json), "\"when\": \"after the failure (reads only)\""));

    /* Success, and a failure before anything ran (summary set by the caller). */
    memset(&s, 0, sizeof(s));
    s.state = "ok";
    s.mode = "run";
    s.fail_class = "";
    s.seconds = 95.8;
    s.nscripts = 1;
    s.script[0].path = "/p/init.mdio";
    s.script[0].started = 1;
    s.script[0].parsed.ops = s.script[0].run.ops = 963356;
    s.report = &rep;
    mdio_status_summarize(&s);
    CHECK(!strcmp(s.summary, "2 PHYs verified after 963356 operations in 95.8 s") && !s.hint[0]);
    CHECK(mdio_status_write(json, &s) == 0);
    CHECK(strncmp(slurp(txt), "openbcm-mdio-script: ok: 2 PHYs verified", 40) == 0);
    CHECK(has(slurp(json), "\"failure\": null") && has(slurp(json), "\"state\": \"ok\""));
    memset(&s, 0, sizeof(s));
    s.state = "failed";
    s.mode = "run";
    s.fail_class = MDIO_CLASS_SDK_BUSY;
    snprintf(s.summary, sizeof(s.summary), "the SDK lock /run/openbcm-unit0.lock is held by pid 812 (syncd)");
    mdio_status_summarize(&s);
    CHECK(has(s.summary, "held by pid 812 (syncd)") && has(s.hint, "stop the process"));
    unlink(json);
    unlink(txt);
    rmdir(dir);
}

static void test_system(const char* exe)
{
    char err[300], comm[32], *out, lock[64], script[64], names[64];
    unsigned v = 0;
    int pid = 0, fd, lfd, rc;
    struct sockaddr_un a;
    pthread_t th;
    FILE* f;

    /* Who holds an flock (the SDK lock): this process. */
    snprintf(lock, sizeof(lock), "/tmp/test_mdio_lock.%d", (int)getpid());
    fd = open(lock, O_RDWR | O_CREAT, 0600);
    CHECK(mdio_lock_holder(lock, &pid, comm, sizeof(comm)) == 0);
    CHECK(flock(fd, LOCK_EX | LOCK_NB) == 0);
    if (access("/proc/locks", R_OK) == 0) {
        CHECK(mdio_lock_holder(lock, &pid, comm, sizeof(comm)) == 1 && pid == (int)getpid() &&
              strncmp(comm, "test_mdio_scri", 14) == 0);
    } else {
        printf("test_mdio_script: no /proc/locks here, lock holder not tested\n");
    }
    close(fd);
    unlink(lock);
    CHECK(mdio_lock_holder("/nonexistent/lock", &pid, comm, sizeof(comm)) == 0);

    /* Reads through (a fake of) syncd's diag socket. */
    snprintf(sock_path, sizeof(sock_path), "/tmp/test_mdio_diag.%d.sock", (int)getpid());
    unlink(sock_path);
    CHECK(mdio_diag_c45_read(sock_path, 0, 1, 0x8b00, &v, err, sizeof(err)) != 0 && has(err, "is syncd running"));
    memset(&a, 0, sizeof(a));
    a.sun_family = AF_UNIX;
    strcpy(a.sun_path, sock_path);
    lfd = socket(AF_UNIX, SOCK_STREAM, 0);
    CHECK(bind(lfd, (struct sockaddr*)&a, sizeof(a)) == 0 && listen(lfd, 8) == 0);
    pthread_create(&th, NULL, diag_server, &lfd);
    CHECK(mdio_diag_c45_read(sock_path, 0x17f, 1, 0x8b00, &v, err, sizeof(err)) == 0 && v == 0x2391);
    CHECK(mdio_diag_c45_read(sock_path, 0x25, 1, 0x8215, &v, err, sizeof(err)) == 0 && v == 0x0001);
    CHECK(mdio_diag_c45_read(sock_path, 0x26, 1, 0x8215, &v, err, sizeof(err)) != 0 && has(err, "openbcm-diag: command failed"));
    CHECK(mdio_diag_c45_read(sock_path, 0, 0, 2, &v, err, sizeof(err)) != 0 && has(err, "clause 45 only"));

    if (exe) {
        char dir[64], json[100], txt[100];
        snprintf(script, sizeof(script), "/tmp/test_mdio_cli.%d.mdio", (int)getpid());
        snprintf(names, sizeof(names), "/tmp/test_mdio_cli.%d.names", (int)getpid());
        snprintf(dir, sizeof(dir), "/tmp/test_mdio_cli.%d.d", (int)getpid());
        snprintf(json, sizeof(json), "%s/status.json", dir);
        snprintf(txt, sizeof(txt), "%s/status.txt", dir);
        f = fopen(names, "w");
        fputs("phy 0 cage1\nphy 1 cage2\nphy 24 cage5\nreg 1.8b00 id\nreg 1.8215 fw\n", f);
        fclose(f);
        f = fopen(script, "w");
        fputs(BODY VERIFY(0) VERIFY(1), f);
        fclose(f);
        /* --check */
        rc = tool(exe, &out, "--check", script, NULL, NULL, NULL, NULL, NULL);
        CHECK(rc == 0 && has(out, "3 writes") && has(out, "verification: 8 reads from line 8"));
        /* read-only verification through the diag socket: all good */
        rc = tool(exe, &out, "--verify-only", "--diag-socket", sock_path, "--names", names, script, NULL);
        CHECK(rc == 0 && has(out, "cage1  0x000  2391    80a0    d00b    002d    ok\n") &&
              has(out, "cage2  0x001  2391") && has(out, "2 of 2 PHYs as the script expects"));
        bus_writes = 0;
        rc = tool(exe, &out, "--verify-only", "--diag-socket", sock_path, "--json", script, NULL, NULL);
        CHECK(rc == 0 && has(out, "\"mode\": \"verify\"") && has(out, "\"phys_ok\": 2, \"phys_total\": 2") &&
              has(out, "\"failure\": null"));
        /* a hung PHY: exit 2, the row and the class say so, the board's hint is used */
        f = fopen(script, "w");
        fputs(BODY VERIFY(0) VERIFY(24), f);
        fclose(f);
        rc = tool(exe, &out, "--verify-only", "--diag-socket", sock_path, "--names", names, "--hint",
                  "phy-not-answering=cold power cycle (lab plug 1)");
        CHECK(rc == 1);   /* no script given: usage */
        {
            char hint[] = "phy-not-answering=cold power cycle (lab plug 1)";
            pid_t p2;
            int status = 0;
            char outp[64];
            snprintf(outp, sizeof(outp), "/tmp/test_mdio_script.out2.%d", (int)getpid());
            p2 = fork();
            if (p2 == 0) {
                int ofd = open(outp, O_WRONLY | O_CREAT | O_TRUNC, 0600);
                dup2(ofd, 1);
                dup2(ofd, 2);
                execl(exe, exe, "--verify-only", "--diag-socket", sock_path, "--names", names, "--hint", hint, script,
                      (char*)NULL);
                _exit(127);
            }
            waitpid(p2, &status, 0);
            out = slurp(outp);
            unlink(outp);
            CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 2);
            CHECK(has(out, "cage5  0x024  1900!   1900!   1900!   1900!   CONSTANT 0x1900: not answering or hung\n") &&
                  has(out, "1 of 2 PHYs are not as the script expects (phy-not-answering)") &&
                  has(out, "what to do: cold power cycle (lab plug 1)"));
        }
        /* --verify-only never writes the status file, and needs a socket or a config */
        rc = tool(exe, &out, "--verify-only", script, NULL, NULL, NULL, NULL, NULL);
        CHECK(rc == 1 && has(out, "usage:"));
        /* no verification section */
        f = fopen(script, "w");
        fputs("mdio-script 1\nW 0 1 1 1\n", f);
        fclose(f);
        rc = tool(exe, &out, "--verify-only", "--diag-socket", sock_path, script, NULL, NULL, NULL);
        CHECK(rc == 5 && has(out, "no script has a verification section"));
        /* a run with a missing script: exit 5, both status files say why, nothing else happened */
        unlink(script);
        rc = tool(exe, &out, "--config", "/nonexistent/config.bcm", "--status-file", json, "--hint",
                  "script-missing=run install-script.sh", script);
        CHECK(rc == 5 && has(out, "FAILED (script-missing): ") && has(out, "No such file or directory; nothing was written to any PHY") &&
              has(out, "what to do: run install-script.sh"));
        CHECK(has(slurp(json), "\"class\": \"script-missing\", \"hint\": \"run install-script.sh\"") &&
              has(slurp(json), "\"state\": \"failed\"") && has(slurp(json), "\"verified\": null"));
        CHECK(strncmp(slurp(txt), "openbcm-mdio-script: FAILED (script-missing): ", 46) == 0);
        /* a truncated one */
        f = fopen(script, "w");
        fputs("mdio-script 1\nW 0 1 1 1\nW 0 1 2\n", f);
        fclose(f);
        rc = tool(exe, &out, "--config", "/nonexistent/config.bcm", "--status-file", json, script, NULL, NULL);
        CHECK(rc == 5 && has(out, "FAILED (script-malformed): ") && has(out, ":3: bad arguments") &&
              has(out, "what to do: the file is truncated"));
        CHECK(has(slurp(json), "\"class\": \"script-malformed\""));
        unlink(json);
        unlink(txt);
        rmdir(dir);
        unlink(script);
        unlink(names);
    } else {
        printf("test_mdio_script: no tool path given, command line not tested\n");
    }
    shutdown(lfd, SHUT_RDWR);
    close(lfd);
    pthread_join(th, NULL);
    unlink(sock_path);
}

int main(int argc, char** argv)
{
    mdio_script_stats_t st;
    char err[256];

    /* order, values, delay, labels */
    memset(regs, 0, sizeof(regs));
    regs[0x8b00] = 0x2391;
    CHECK(run("mdio-script 1\n# c\nL step one\nW 17c 1 8201 7e\nD 30\nR 17c 1 900a\nE 17c 1 8b00 2391\n"
              "E 17c 1 8b00 0391 0fff\nE 17c 1 8b00 1111\n\nW 0 1f ffff ffff\n", 1, &st, err) == MDIO_SCRIPT_OK);
    CHECK(strcmp(trace, "W17c.1.8201=7e R17c.1.900a R17c.1.8b00 R17c.1.8b00 R17c.1.8b00 W0.1f.ffff=ffff ") == 0);
    CHECK(st.writes == 2 && st.reads == 4 && st.delays == 1 && st.delay_us == 30 && st.labels == 1 && st.differs == 1);
    CHECK(slept == 30);
    CHECK(strstr(last_log, "step one") && strstr(last_log, "differs: line 9: 17c 1.8b00 = 2391, script 1111"));

    /* P: re-read until the value, with the interval; N: until it changes */
    ready_after = 3;
    CHECK(run("mdio-script 1\nP 24 1 8221 0303 ffff 10 1000\n", 1, &st, err) == MDIO_SCRIPT_OK);
    CHECK(st.polls == 1 && st.poll_rereads == 3 && st.reads == 4 && slept == 3000);
    ready_after = 2;
    CHECK(run("mdio-script 1\nN 24 1 8221 0 ffff 10 500\n", 1, &st, err) == MDIO_SCRIPT_OK);
    CHECK(st.poll_rereads == 2 && slept == 1000);
    /* masked poll, first read */
    regs[0x8251] = 0x002d;
    CHECK(run("mdio-script 1\nP 24 1 8251 d d 1 0\n", 1, &st, err) == MDIO_SCRIPT_OK && st.poll_rereads == 0);

    /* a poll that never matches fails the script, names line, label and value; nothing after it runs */
    ready_after = 100;
    CHECK(run("mdio-script 1\nL download bus 1\nP 24 1 8221 0303 ffff 5 10\nW 24 1 1 1\n", 1, &st, err) ==
          MDIO_SCRIPT_E_POLL);
    CHECK(strstr(err, ":3:") && strstr(err, "download bus 1") && strstr(err, "8221") && strstr(err, "after 5 reads"));
    CHECK(strstr(trace, "W24.1.1=1") == NULL && st.writes == 0);
    /* assertion form (tries 1) */
    regs[0x8215] = 1;
    CHECK(run("mdio-script 1\nL verify\nP 24 1 8215 d00b ffff 1 0\n", 1, &st, err) == MDIO_SCRIPT_E_POLL);
    CHECK(strstr(err, "reads 0001") && strstr(err, "d00b"));
    /* I/O error */
    CHECK(run("mdio-script 1\nR 24 1 dead\nW 24 1 1 1\n", 1, &st, err) == MDIO_SCRIPT_E_IO && st.writes == 0);

    /* parse only: no access, same statistics */
    CHECK(run("mdio-script 1\nW 1 1 2 3\nP 1 1 2 3 ffff 4 5\nD 100\n", 0, &st, err) == MDIO_SCRIPT_OK);
    CHECK(io_calls == 0 && slept == 0 && st.writes == 1 && st.polls == 1 && st.delay_us == 100);

    /* refused files */
    CHECK(mdio_script_run("/nonexistent/x.mdio", &io, &st, err, sizeof(err)) == MDIO_SCRIPT_E_FILE);
    CHECK(strstr(err, "/nonexistent/x.mdio") != NULL);
    CHECK(run("", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);
    CHECK(run("W 1 1 2 3\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);                      /* no header */
    CHECK(run("mdio-script 2\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);
    CHECK(run("mdio-script 1\nW 1 0 2 3\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);      /* clause 22 */
    CHECK(run("mdio-script 1\nW 1 20 2 3\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);     /* dev > 31 */
    CHECK(run("mdio-script 1\nW 1 1 2\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);        /* truncated line */
    CHECK(run("mdio-script 1\nW 1 1 2 10000\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);
    CHECK(run("mdio-script 1\nW 1 1 2 3 4\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);
    CHECK(run("mdio-script 1\nX 1 1 2 3\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);
    CHECK(run("mdio-script 1\nP 1 1 2 3 ffff 0 5\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);  /* 0 tries */
    CHECK(run("mdio-script 1\nD -5\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX);
    CHECK(run("mdio-script 1\nW 1 1 2 3\nW 1 1 zz 3\n", 0, &st, err) == MDIO_SCRIPT_E_SYNTAX && strstr(err, ":3:"));
    /* a syntax error found while running stops there (callers parse first) */
    CHECK(run("mdio-script 1\nW 1 1 2 3\nbogus\nW 1 1 4 5\n", 1, &st, err) == MDIO_SCRIPT_E_SYNTAX && st.writes == 1);

    test_verification();
    test_report_and_status();
    test_system(argc > 1 ? argv[1] : NULL);

    unlink(put(""));
    if (failures) {
        fprintf(stderr, "test_mdio_script: %d failures\n", failures);
        return 1;
    }
    printf("test_mdio_script: ok\n");
    return 0;
}
