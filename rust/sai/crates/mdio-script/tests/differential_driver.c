/* SPDX-License-Identifier: Apache-2.0
 * Test input of tests/differential.rs: runs the C interpreter (mdio_script.c, compiled together
 * with this file) on a script against a deterministic fake bus and prints a canonical,
 * line-oriented result. tests/common/mod.rs holds the same bus and the same printing in Rust.
 *
 *   driver <script> <parse|run|verify|only> <seed> <tick 0|1> <abort_k>
 *
 * Bus rules: a read fails when reg == dead or phy == 26; a write fails when reg == bad0;
 * reg 8221 reads 0 for the first (seed % 7) reads of it, then 0303; phy 24 reads 1900 on every
 * other register; any other register reads what was written to it, or a hash of seed and
 * address in 0..3. The sleep only prints. The tick prints its count and the running st.ops and
 * asks to stop from call abort_k on (0 = never). */
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mdio_script.h"

static uint32_t seed;
static long ready_after;
static unsigned long ticks, abort_k;
static mdio_script_stats_t st;

#define MAX_WRITTEN 4096
static struct {
    unsigned phy, dev, reg, val;
} written[MAX_WRITTEN];
static int nwritten;

static void esc(const char* s)
{
    for (; *s; s++) {
        unsigned char c = (unsigned char)*s;
        if (c >= 0x20 && c < 0x7f && c != '\\') putchar(c);
        else printf("\\x%02x", c);
    }
}

static unsigned initial(unsigned phy, unsigned dev, unsigned reg)
{
    uint32_t h = seed * 2654435761u + (uint32_t)phy * 40503u + (uint32_t)dev * 8191u + (uint32_t)reg * 131u;
    h ^= h >> 13;
    h *= 0x5bd1e995u;
    h ^= h >> 15;
    return h & 3;
}

static int b_read(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned* val)
{
    int i;
    (void)c;
    if (reg == 0xdead || phy == 0x26) {
        printf("R %x %x %x ERR\n", phy, dev, reg);
        return -1;
    }
    if (reg == 0x8221) {
        *val = ready_after-- > 0 ? 0 : 0x0303;
    } else if (phy == 0x24) {
        *val = 0x1900;
    } else {
        *val = initial(phy, dev, reg);
        for (i = 0; i < nwritten; i++)
            if (written[i].phy == phy && written[i].dev == dev && written[i].reg == reg) *val = written[i].val;
    }
    printf("R %x %x %x = %04x\n", phy, dev, reg, *val);
    return 0;
}

static int b_write(void* c, unsigned phy, unsigned dev, unsigned reg, unsigned val)
{
    int i;
    (void)c;
    if (reg == 0xbad0) {
        printf("W %x %x %x %x ERR\n", phy, dev, reg, val);
        return -1;
    }
    for (i = 0; i < nwritten; i++)
        if (written[i].phy == phy && written[i].dev == dev && written[i].reg == reg) break;
    if (i == MAX_WRITTEN) {
        fprintf(stderr, "driver: more than %d registers written\n", MAX_WRITTEN);
        exit(3);
    }
    if (i == nwritten) nwritten++;
    written[i].phy = phy;
    written[i].dev = dev;
    written[i].reg = reg;
    written[i].val = val;
    printf("W %x %x %x %x\n", phy, dev, reg, val);
    return 0;
}

static void b_sleep(void* c, unsigned long us)
{
    (void)c;
    printf("S %lu\n", us);
}

static void b_log(void* c, const char* m)
{
    (void)c;
    printf("L ");
    esc(m);
    putchar('\n');
}

static int b_tick(void* c)
{
    (void)c;
    ticks++;
    printf("T %lu ops=%lu\n", ticks, st.ops);
    return abort_k && ticks >= abort_k;
}

static void b_verify(void* c, const mdio_script_check_t* k)
{
    (void)c;
    printf("V %lu %c %x %x %x %04x %04x %04x ok=%d io=%d ", k->line, k->op, k->phy, k->dev, k->reg, k->want, k->mask,
           k->got, k->ok, k->io_error);
    esc(k->label);
    putchar('\n');
}

/* "<path>:<line>: text" -> "line <line>: text"; "<path>: text" -> "text". */
static void result(const char* tag, int rc, const char* path, const char* err)
{
    size_t n = strlen(path);
    printf("%s rc=%d lines=%lu writes=%lu reads=%lu delays=%lu polls=%lu labels=%lu poll_rereads=%lu differs=%lu "
           "delay_us=%llu ops=%lu verify_line=%lu verify_checks=%lu verify_failed=%lu\n",
           tag, rc, st.lines, st.writes, st.reads, st.delays, st.polls, st.labels, st.poll_rereads, st.differs,
           st.delay_us, st.ops, st.verify_line, st.verify_checks, st.verify_failed);
    printf("%s err=", tag);
    if (strncmp(err, path, n) == 0 && err[n] == ':') {
        if (isdigit((unsigned char)err[n + 1])) {
            printf("line ");
            esc(err + n + 1);
        } else {
            esc(err + n + (err[n + 1] == ' ' ? 2 : 1));
        }
    } else {
        esc(err);
    }
    putchar('\n');
}

int main(int argc, char** argv)
{
    mdio_script_io_t io = {b_read, b_write, b_sleep, b_log, NULL, NULL, b_verify};
    mdio_script_fail_t fail;
    mdio_script_opts_t o = {0, 0, &fail};
    char err[2048];
    const char *path, *mode;
    int rc;

    if (argc != 6) {
        fprintf(stderr, "usage: %s <script> <parse|run|verify|only> <seed> <tick 0|1> <abort_k>\n", argv[0]);
        return 2;
    }
    path = argv[1];
    mode = argv[2];
    seed = (uint32_t)strtoul(argv[3], NULL, 10);
    if (atoi(argv[4])) io.tick = b_tick;
    abort_k = strtoul(argv[5], NULL, 10);
    ready_after = (long)(seed % 7);

    rc = mdio_script_run(path, NULL, &st, err, sizeof(err));
    result("parse", rc, path, err);
    if (rc != MDIO_SCRIPT_OK || !strcmp(mode, "parse")) return 0;

    if (!strcmp(mode, "verify") || !strcmp(mode, "only")) o.verify_line = st.verify_line;
    if (!strcmp(mode, "only")) o.verify_only = 1;
    rc = mdio_script_run_opts(path, &io, &o, &st, err, sizeof(err));
    result("run", rc, path, err);
    printf("fail line=%lu op=%c phy=%x dev=%x reg=%x want=%x mask=%x got=%x tries=%lu have=%d label=", fail.line,
           fail.op ? fail.op : '-', fail.phy, fail.dev, fail.reg, fail.want, fail.mask, fail.got, fail.tries,
           fail.have_value);
    esc(fail.label);
    putchar('\n');
    return 0;
}
