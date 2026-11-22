/* SPDX-License-Identifier: Apache-2.0
 * Unit test of the MDIO script interpreter (src/mdio_script.c) on a fake bus. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

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
static const mdio_script_io_t io = {f_read, f_write, f_sleep, f_log, NULL};

static const char* put(const char* text)
{
    static char path[64];
    FILE* f;
    snprintf(path, sizeof(path), "/tmp/test_mdio_script.%d", (int)getpid());
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

int main(void)
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

    unlink(put(""));
    if (failures) {
        fprintf(stderr, "test_mdio_script: %d failures\n", failures);
        return 1;
    }
    printf("test_mdio_script: ok\n");
    return 0;
}
