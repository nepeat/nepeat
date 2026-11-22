/* SPDX-License-Identifier: Apache-2.0
 * See mdio_report.h. Plain C, no SDK code: built and tested on any host. */
#define _GNU_SOURCE
#include "mdio_report.h"

#include <errno.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

/* ------------------------------------------------------------ PHY report */

void mdio_report_reset(mdio_report_t* r) { memset(r, 0, sizeof(*r)); }

void mdio_report_add(mdio_report_t* r, const mdio_script_check_t* c)
{
    mdio_report_phy_t* p = NULL;
    int i;
    for (i = 0; i < r->nphy; i++)
        if (r->phy[i].phy == c->phy) p = &r->phy[i];
    if (!p) {
        if (r->nphy == MDIO_REPORT_MAX_PHYS) {
            r->dropped++;
            return;
        }
        p = &r->phy[r->nphy++];
        memset(p, 0, sizeof(*p));
        p->phy = c->phy;
    }
    if (p->n == MDIO_REPORT_MAX_CHECKS) {
        r->dropped++;
        if (!c->ok && p->state == MDIO_PHY_OK) p->state = MDIO_PHY_MISMATCH;   /* never lose a failure */
        return;
    }
    p->c[p->n].dev = c->dev;
    p->c[p->n].reg = c->reg;
    p->c[p->n].want = c->want;
    p->c[p->n].mask = c->mask;
    p->c[p->n].got = c->got;
    p->c[p->n].op = c->op;
    p->c[p->n].ok = c->ok;
    p->c[p->n].io_error = c->io_error;
    p->n++;
}

void mdio_report_classify(mdio_report_t* r)
{
    int i, k;
    for (i = 0; i < r->nphy; i++) {
        mdio_report_phy_t* p = &r->phy[i];
        int bad = p->state == MDIO_PHY_MISMATCH, io = 0, same = 1;
        for (k = 0; k < p->n; k++) {
            if (!p->c[k].ok) bad = 1;
            if (p->c[k].io_error) io = 1;
            if (p->c[k].got != p->c[0].got) same = 0;
        }
        if (io) p->state = MDIO_PHY_IO;
        else if (!bad) p->state = MDIO_PHY_OK;
        else if (p->n >= 3 && same) {
            p->state = MDIO_PHY_CONSTANT;
            p->constant = p->c[0].got;
        } else p->state = MDIO_PHY_MISMATCH;
    }
}

int mdio_report_count(const mdio_report_t* r, int state)
{
    int i, n = 0;
    for (i = 0; i < r->nphy; i++)
        if (r->phy[i].state == state) n++;
    return n;
}

const mdio_report_phy_t* mdio_report_find(const mdio_report_t* r, unsigned phy)
{
    int i;
    for (i = 0; r && i < r->nphy; i++)
        if (r->phy[i].phy == phy) return &r->phy[i];
    return NULL;
}

const char* mdio_phy_state_name(int state)
{
    switch (state) {
    case MDIO_PHY_OK: return "ok";
    case MDIO_PHY_MISMATCH: return "mismatch";
    case MDIO_PHY_CONSTANT: return "constant";
    default: return "io-error";
    }
}

int mdio_names_load(const char* path, mdio_names_t* n, char* err, int errlen)
{
    char line[200], kind[8], a[32], name[64];
    unsigned long ln = 0;
    FILE* f = fopen(path, "r");
    memset(n, 0, sizeof(*n));
    if (!f) {
        snprintf(err, (size_t)errlen, "%s: %s", path, strerror(errno));
        return -1;
    }
    while (fgets(line, sizeof(line), f)) {
        char* end;
        ln++;
        if (line[0] == '#' || sscanf(line, "%7s %31s %63s", kind, a, name) < 1) continue;
        if (!strcmp(kind, "phy") && n->nphy < MDIO_REPORT_MAX_PHYS && sscanf(line, "%*s %31s %63s", a, name) == 2) {
            n->p[n->nphy].phy = (unsigned)strtoul(a, &end, 16);
            if (*end) goto bad;
            snprintf(n->p[n->nphy++].name, sizeof(n->p[0].name), "%.23s", name);
        } else if (!strcmp(kind, "reg") && n->nreg < 32 && sscanf(line, "%*s %31s %63s", a, name) == 2) {
            n->r[n->nreg].dev = (unsigned)strtoul(a, &end, 16);
            if (*end != '.') goto bad;
            n->r[n->nreg].reg = (unsigned)strtoul(end + 1, &end, 16);
            if (*end) goto bad;
            snprintf(n->r[n->nreg++].name, sizeof(n->r[0].name), "%.11s", name);
        } else {
            goto bad;
        }
    }
    fclose(f);
    return 0;
bad:
    fclose(f);
    snprintf(err, (size_t)errlen, "%s:%lu: want 'phy <hex> <name>' or 'reg <dev>.<reg> <name>'", path, ln);
    return -1;
}

const char* mdio_names_phy(const mdio_names_t* n, unsigned phy)
{
    int i;
    for (i = 0; n && i < n->nphy; i++)
        if (n->p[i].phy == phy) return n->p[i].name;
    return NULL;
}

const char* mdio_names_reg(const mdio_names_t* n, unsigned dev, unsigned reg)
{
    int i;
    for (i = 0; n && i < n->nreg; i++)
        if (n->r[i].dev == dev && n->r[i].reg == reg) return n->r[i].name;
    return NULL;
}

/* "cage5 (phy 0x24)" or "phy 0x24". */
static const char* phy_label(const mdio_names_t* names, unsigned phy, char* b, size_t n)
{
    const char* nm = mdio_names_phy(names, phy);
    if (nm) snprintf(b, n, "%s (phy 0x%x)", nm, phy);
    else snprintf(b, n, "phy 0x%x", phy);
    return b;
}

static const char* reg_label(const mdio_names_t* names, unsigned dev, unsigned reg, char* b, size_t n)
{
    const char* nm = mdio_names_reg(names, dev, reg);
    if (nm) snprintf(b, n, "%s (%x.%04x)", nm, dev, reg);
    else snprintf(b, n, "%x.%04x", dev, reg);
    return b;
}

static void state_text(const mdio_report_phy_t* p, char* b, size_t n)
{
    switch (p->state) {
    case MDIO_PHY_OK: snprintf(b, n, "ok"); break;
    case MDIO_PHY_CONSTANT: snprintf(b, n, "CONSTANT 0x%04x: not answering or hung", p->constant); break;
    case MDIO_PHY_MISMATCH: snprintf(b, n, "MISMATCH"); break;
    default: snprintf(b, n, "MDIO ERROR"); break;
    }
}

void mdio_report_table(FILE* f, const mdio_report_t* r, const mdio_names_t* names)
{
    struct { unsigned dev, reg; } col[MDIO_REPORT_MAX_CHECKS];
    int ncol = 0, i, k, c, w = 4;
    char b[64];
    for (i = 0; i < r->nphy; i++) {
        const char* nm = mdio_names_phy(names, r->phy[i].phy);
        if (nm && (int)strlen(nm) > w) w = (int)strlen(nm);
        for (k = 0; k < r->phy[i].n; k++) {
            for (c = 0; c < ncol; c++)
                if (col[c].dev == r->phy[i].c[k].dev && col[c].reg == r->phy[i].c[k].reg) break;
            if (c == ncol && ncol < MDIO_REPORT_MAX_CHECKS) {
                col[ncol].dev = r->phy[i].c[k].dev;
                col[ncol++].reg = r->phy[i].c[k].reg;
            }
        }
    }
    fprintf(f, "%-*s  %-6s", w, "name", "phy");
    for (c = 0; c < ncol; c++) {
        const char* nm = mdio_names_reg(names, col[c].dev, col[c].reg);
        if (nm) snprintf(b, sizeof(b), "%s", nm);
        else snprintf(b, sizeof(b), "%x.%04x", col[c].dev, col[c].reg);
        fprintf(f, " %-7.7s", b);
    }
    fprintf(f, " state\n");
    for (i = 0; i < r->nphy; i++) {
        const mdio_report_phy_t* p = &r->phy[i];
        const char* nm = mdio_names_phy(names, p->phy);
        fprintf(f, "%-*s  0x%03x ", w, nm ? nm : "-", p->phy);
        for (c = 0; c < ncol; c++) {
            for (k = 0; k < p->n; k++)
                if (p->c[k].dev == col[c].dev && p->c[k].reg == col[c].reg) break;
            if (k == p->n) fprintf(f, " %-7s", "-");
            else if (p->c[k].io_error) fprintf(f, " %-7s", "error!");
            else fprintf(f, " %04x%-3s", p->c[k].got, p->c[k].ok ? "" : "!");
        }
        state_text(p, b, sizeof(b));
        fprintf(f, " %s\n", b);
    }
    if (r->dropped) fprintf(f, "(%d verification reads beyond the table size are not shown)\n", r->dropped);
}

/* --------------------------------------------------------- classification */

const char* mdio_classify_run(int rv, const mdio_script_fail_t* fail, const mdio_report_t* post)
{
    const mdio_report_phy_t* p;
    switch (rv) {
    case MDIO_SCRIPT_E_FILE: return MDIO_CLASS_SCRIPT_MISSING;
    case MDIO_SCRIPT_E_SYNTAX: return MDIO_CLASS_SCRIPT_MALFORMED;
    case MDIO_SCRIPT_E_IO: return MDIO_CLASS_MDIO_IO;
    case MDIO_SCRIPT_E_ABORT: return MDIO_CLASS_INTERRUPTED;
    case MDIO_SCRIPT_E_POLL:
        /* The PHY the script stopped at decides; when it is not in the
         * verification list, any PHY that reads a constant does. */
        p = fail ? mdio_report_find(post, fail->phy) : NULL;
        if (p ? p->state == MDIO_PHY_CONSTANT : (post && mdio_report_count(post, MDIO_PHY_CONSTANT) > 0))
            return MDIO_CLASS_PHY_CONSTANT;
        return MDIO_CLASS_DIVERGED;
    case MDIO_SCRIPT_E_VERIFY:
        if (post && mdio_report_count(post, MDIO_PHY_CONSTANT) > 0) return MDIO_CLASS_PHY_CONSTANT;
        if (post && mdio_report_count(post, MDIO_PHY_IO) > 0) return MDIO_CLASS_MDIO_IO;
        return MDIO_CLASS_VERIFY;
    default: return "";
    }
}

const char* mdio_class_hint(const char* cls)
{
    static const struct { const char *cls, *hint; } h[] = {
        {MDIO_CLASS_SCRIPT_MISSING, "install the script file (private data, copied by hand), then start the unit again"},
        {MDIO_CLASS_SCRIPT_MALFORMED, "the file is truncated or not an MDIO script: install it again"},
        {MDIO_CLASS_SDK_BUSY, "stop the process that owns the switch chip, then start the unit again"},
        {MDIO_CLASS_SDK_CONFIG, "the SDK config file given with --config is missing: a build or deployment problem"},
        {MDIO_CLASS_SDK_INIT, "the switch chip did not initialise: see the SDK messages before this line in the journal"},
        {MDIO_CLASS_MDIO_CLOCK,
         "the SDK config must set rate_ext_mdio_divisor and mdio_output_delay; no MDIO frame was sent"},
        {MDIO_CLASS_MDIO_IO, "the switch chip's MDIO master failed (not a PHY): see the SDK messages in the journal"},
        {MDIO_CLASS_PHY_CONSTANT, "a PHY that reads one constant on every register does not recover by itself: "
                                  "power-cycle the switch (cold), then let the unit run again"},
        {MDIO_CLASS_DIVERGED, "start the unit again; if it stops at the same step, the script does not match this "
                              "hardware or its firmware"},
        {MDIO_CLASS_VERIFY, "start the unit again; a PHY that keeps differing is not in the state the script was "
                            "recorded for"},
        {MDIO_CLASS_INTERRUPTED, "the PHYs may be half-programmed: start the unit again"},
    };
    size_t i;
    for (i = 0; cls && i < sizeof(h) / sizeof(h[0]); i++)
        if (!strcmp(h[i].cls, cls)) return h[i].hint;
    return "";
}

/* ----------------------------------------------------------------- status */

static void app(char* b, size_t cap, const char* fmt, ...) __attribute__((format(printf, 3, 4)));
static void app(char* b, size_t cap, const char* fmt, ...)
{
    size_t len = strlen(b);
    va_list ap;
    if (len + 1 >= cap) return;
    va_start(ap, fmt);
    vsnprintf(b + len, cap - len, fmt, ap);
    va_end(ap);
}

static const char* base(const char* p)
{
    const char* s = p ? strrchr(p, '/') : NULL;
    return s ? s + 1 : (p ? p : "");
}

static void totals(const mdio_status_t* st, unsigned long* done, unsigned long* all)
{
    int i;
    *done = *all = 0;
    for (i = 0; i < st->nscripts; i++) {
        *all += st->script[i].parsed.ops;
        if (st->script[i].started) *done += st->script[i].run.ops;
    }
}

void mdio_status_summarize(mdio_status_t* st)
{
    char b[80], rb[40];
    unsigned long done, all;
    const mdio_script_fail_t* f = &st->fail;
    const mdio_report_t* r = st->report;
    int i, k, shown = 0, rv = MDIO_SCRIPT_OK;
    totals(st, &done, &all);
    for (i = 0; i < st->nscripts; i++)
        if (st->script[i].started && st->script[i].result != MDIO_SCRIPT_OK) rv = st->script[i].result;
    if (st->fail_class && *st->fail_class && !st->hint[0])
        snprintf(st->hint, sizeof(st->hint), "%s", mdio_class_hint(st->fail_class));
    if (st->summary[0]) return;   /* set by the caller (nothing ran) */
    if (!strcmp(st->state, "ok")) {
        if (!strcmp(st->mode, "verify"))
            snprintf(st->summary, sizeof(st->summary), "%d PHYs verified, all as the script expects", r ? r->nphy : 0);
        else if (r && r->nphy)
            snprintf(st->summary, sizeof(st->summary), "%d PHYs verified after %lu operations in %.1f s", r->nphy, done,
                     st->seconds);
        else
            snprintf(st->summary, sizeof(st->summary),
                     "%lu operations in %.1f s; the script has no verification section", done, st->seconds);
        return;
    }
    if (!strcmp(st->state, "running")) {
        snprintf(st->summary, sizeof(st->summary), "%lu of %lu operations done", done, all);
        return;
    }
    /* failed / interrupted while a script ran */
    if (f->line) app(st->summary, sizeof(st->summary), "%s line %lu [%s]: ", base(st->fail_script), f->line, f->label);
    if (!strcmp(st->fail_class, MDIO_CLASS_INTERRUPTED)) {
        app(st->summary, sizeof(st->summary), "stopped on request (signal)");
    } else if (!strcmp(st->fail_class, MDIO_CLASS_MDIO_IO) && f->op) {
        app(st->summary, sizeof(st->summary), "MDIO %s of %s %s failed in the switch chip's MDIO master",
            f->op == 'W' ? "write" : "read", phy_label(st->names, f->phy, b, sizeof(b)),
            reg_label(st->names, f->dev, f->reg, rb, sizeof(rb)));
    } else if (rv == MDIO_SCRIPT_E_VERIFY && r && r->nphy) {
        /* verification */
        app(st->summary, sizeof(st->summary), "verification: %d of %d PHYs differ: ",
            r->nphy - mdio_report_count(r, MDIO_PHY_OK), r->nphy);
        for (i = 0; i < r->nphy && shown < 3; i++) {
            const mdio_report_phy_t* p = &r->phy[i];
            if (p->state == MDIO_PHY_OK) continue;
            app(st->summary, sizeof(st->summary), "%s%s ", shown++ ? "; " : "", phy_label(st->names, p->phy, b, sizeof(b)));
            if (p->state == MDIO_PHY_CONSTANT) {
                app(st->summary, sizeof(st->summary), "reads 0x%04x on every verified register", p->constant);
                continue;
            }
            for (k = 0; k < p->n; k++)
                if (!p->c[k].ok) break;
            if (k == p->n) app(st->summary, sizeof(st->summary), "differs");
            else if (p->c[k].io_error)
                app(st->summary, sizeof(st->summary), "%s: read failed", reg_label(st->names, p->c[k].dev, p->c[k].reg, rb, sizeof(rb)));
            else
                app(st->summary, sizeof(st->summary), "%s read 0x%04x, expected %s0x%04x%s",
                    reg_label(st->names, p->c[k].dev, p->c[k].reg, rb, sizeof(rb)), p->c[k].got,
                    p->c[k].op == 'N' ? "anything but " : "", p->c[k].want, p->c[k].mask == 0xffff ? "" : " (masked)");
        }
        if (r->nphy - mdio_report_count(r, MDIO_PHY_OK) > shown) app(st->summary, sizeof(st->summary), "; ...");
    } else if (f->op) {
        app(st->summary, sizeof(st->summary), "%s %s read 0x%04x, the script expects %s0x%04x (mask 0x%04x) after %lu read%s",
            phy_label(st->names, f->phy, b, sizeof(b)), reg_label(st->names, f->dev, f->reg, rb, sizeof(rb)), f->got,
            f->op == 'N' ? "anything but " : "", f->want, f->mask, f->tries, f->tries == 1 ? "" : "s");
    } else {
        app(st->summary, sizeof(st->summary), "failed");
    }
    app(st->summary, sizeof(st->summary), "; %lu of %lu operations had run", done, all);
    if (!strcmp(st->fail_class, MDIO_CLASS_PHY_CONSTANT)) {
        const mdio_report_phy_t* p = mdio_report_find(r, f->phy);
        if (!p || p->state != MDIO_PHY_CONSTANT)
            for (i = 0, p = NULL; r && i < r->nphy && !p; i++)
                if (r->phy[i].state == MDIO_PHY_CONSTANT) p = &r->phy[i];
        if (p)
            app(st->summary, sizeof(st->summary), "; %s reads 0x%04x on every verified register: not answering or hung",
                phy_label(st->names, p->phy, b, sizeof(b)), p->constant);
    } else if (!strcmp(st->fail_class, MDIO_CLASS_DIVERGED)) {
        const mdio_report_phy_t* p = mdio_report_find(r, f->phy);
        app(st->summary, sizeof(st->summary), "; %sit left the recorded sequence",
            p ? "the PHY still answers (its registers are not one constant): " : "");
    } else if (!strcmp(st->fail_class, MDIO_CLASS_VERIFY)) {
        app(st->summary, sizeof(st->summary), "; the PHYs answer, but firmware or configuration is not as recorded");
    }
}

static void jstr(FILE* f, const char* s)
{
    fputc('"', f);
    for (; s && *s; s++) {
        unsigned char ch = (unsigned char)*s;
        if (ch == '"' || ch == '\\') fprintf(f, "\\%c", ch);
        else if (ch < 0x20) fprintf(f, "\\u%04x", ch);
        else fputc(ch, f);
    }
    fputc('"', f);
}

static void jstats(FILE* f, const mdio_script_stats_t* s)
{
    fprintf(f, "{\"operations\": %lu, \"writes\": %lu, \"reads\": %lu, \"polls\": %lu, \"poll_rereads\": %lu, "
               "\"delays\": %lu, \"labels\": %lu, \"expected_values_differed\": %lu, \"verify_checks\": %lu, "
               "\"verify_failed\": %lu}",
            s->ops, s->writes, s->reads, s->polls, s->poll_rereads, s->delays, s->labels, s->differs, s->verify_checks,
            s->verify_failed);
}

static const char* result_name(int rv)
{
    switch (rv) {
    case MDIO_SCRIPT_OK: return "ok";
    case MDIO_SCRIPT_E_FILE: return "file";
    case MDIO_SCRIPT_E_SYNTAX: return "syntax";
    case MDIO_SCRIPT_E_POLL: return "poll";
    case MDIO_SCRIPT_E_IO: return "io";
    case MDIO_SCRIPT_E_VERIFY: return "verify";
    case MDIO_SCRIPT_E_ABORT: return "aborted";
    default: return "?";
    }
}

void mdio_status_json(FILE* f, const mdio_status_t* st)
{
    unsigned long done, all;
    int i, k;
    totals(st, &done, &all);
    fprintf(f, "{\n \"version\": 1,\n \"state\": ");
    jstr(f, st->state);
    fprintf(f, ",\n \"mode\": ");
    jstr(f, st->mode);
    fprintf(f, ",\n \"summary\": ");
    jstr(f, st->summary);
    fprintf(f, ",\n \"pid\": %d,\n \"started\": %lld,\n \"finished\": ", st->pid, st->started);
    if (st->finished) fprintf(f, "%lld", st->finished);
    else fprintf(f, "null");
    fprintf(f, ",\n \"seconds\": %.1f,\n \"operations_done\": %lu,\n \"operations_total\": %lu,\n \"sdk\": ", st->seconds,
            done, all);
    jstr(f, st->sdk);
    fprintf(f, ",\n \"scripts\": [");
    for (i = 0; i < st->nscripts; i++) {
        const mdio_status_script_t* s = &st->script[i];
        fprintf(f, "%s\n  {\"path\": ", i ? "," : "");
        jstr(f, s->path);
        fprintf(f, ", \"result\": ");
        if (s->started) jstr(f, result_name(s->result));
        else fprintf(f, "null");
        fprintf(f, ", \"seconds\": %.1f, \"verify_line\": %lu,\n   \"file\": ", s->seconds, s->parsed.verify_line);
        jstats(f, &s->parsed);
        fprintf(f, ",\n   \"run\": ");
        if (s->started) jstats(f, &s->run);
        else fprintf(f, "null");
        fprintf(f, "}");
    }
    fprintf(f, "\n ],\n \"failure\": ");
    if (st->fail_class && *st->fail_class) {
        const mdio_script_fail_t* x = &st->fail;
        fprintf(f, "{\"class\": ");
        jstr(f, st->fail_class);
        fprintf(f, ", \"hint\": ");
        jstr(f, st->hint);
        fprintf(f, ",\n  \"script\": ");
        if (st->fail_script) jstr(f, st->fail_script);
        else fprintf(f, "null");
        if (x->line) {
            char op[2] = {x->op, 0};
            fprintf(f, ", \"line\": %lu, \"label\": ", x->line);
            jstr(f, x->label);
            fprintf(f, ", \"op\": ");
            jstr(f, op);
            if (x->op) {
                fprintf(f, ",\n  \"phy\": \"0x%x\", \"phy_name\": ", x->phy);
                if (mdio_names_phy(st->names, x->phy)) jstr(f, mdio_names_phy(st->names, x->phy));
                else fprintf(f, "null");
                fprintf(f, ", \"dev\": %u, \"reg\": \"0x%04x\", \"expected\": \"0x%04x\", \"mask\": \"0x%04x\", \"read\": ",
                        x->dev, x->reg, x->want, x->mask);
                if (x->have_value) fprintf(f, "\"0x%04x\"", x->got);
                else fprintf(f, "null");
                fprintf(f, ", \"reads\": %lu", x->tries);
            }
        }
        fprintf(f, "}");
    } else {
        fprintf(f, "null");
    }
    fprintf(f, ",\n \"asic_temperature\": ");
    if (st->temp_enabled)
        fprintf(f, "{\"readings\": %d, \"failed\": %d, \"last_celsius\": %.1f, \"longest_read_ms\": %.2f}", st->temp_reads,
                st->temp_failed, st->temp_last_c, st->temp_max_ms);
    else fprintf(f, "null");
    fprintf(f, ",\n \"verified\": ");
    if (st->report) {
        const mdio_report_t* r = st->report;
        fprintf(f, "{\"when\": ");
        jstr(f, st->report_when ? st->report_when : "");
        fprintf(f, ", \"phys_ok\": %d, \"phys_total\": %d,\n  \"phys\": [", mdio_report_count(r, MDIO_PHY_OK), r->nphy);
        for (i = 0; i < r->nphy; i++) {
            const mdio_report_phy_t* p = &r->phy[i];
            fprintf(f, "%s\n   {\"phy\": \"0x%x\", \"name\": ", i ? "," : "", p->phy);
            if (mdio_names_phy(st->names, p->phy)) jstr(f, mdio_names_phy(st->names, p->phy));
            else fprintf(f, "null");
            fprintf(f, ", \"state\": \"%s\", \"registers\": [", mdio_phy_state_name(p->state));
            for (k = 0; k < p->n; k++) {
                const mdio_report_check_t* c = &p->c[k];
                fprintf(f, "%s{\"reg\": \"%x.%04x\", \"name\": ", k ? ", " : "", c->dev, c->reg);
                if (mdio_names_reg(st->names, c->dev, c->reg)) jstr(f, mdio_names_reg(st->names, c->dev, c->reg));
                else fprintf(f, "null");
                if (c->io_error) fprintf(f, ", \"read\": null");
                else fprintf(f, ", \"read\": \"0x%04x\"", c->got);
                fprintf(f, ", \"expected\": \"%s0x%04x\", \"mask\": \"0x%04x\", \"ok\": %s}", c->op == 'N' ? "!" : "",
                        c->want, c->mask, c->ok ? "true" : "false");
            }
            fprintf(f, "]}");
        }
        fprintf(f, "\n  ]}");
    } else {
        fprintf(f, "null");
    }
    fprintf(f, "\n}\n");
}

static void when(FILE* f, const char* what, long long t)
{
    char b[40];
    time_t tt = (time_t)t;
    struct tm tm;
    if (!t || !localtime_r(&tt, &tm) || !strftime(b, sizeof(b), "%Y-%m-%d %H:%M:%S %z", &tm)) return;
    fprintf(f, "%s %s", what, b);
}

void mdio_status_text(FILE* f, const mdio_status_t* st)
{
    unsigned long done, all;
    int i;
    totals(st, &done, &all);
    if (!strcmp(st->state, "ok")) fprintf(f, "openbcm-mdio-script: ok: %s\n", st->summary);
    else if (!strcmp(st->state, "running")) fprintf(f, "openbcm-mdio-script: running: %s\n", st->summary);
    else
        fprintf(f, "openbcm-mdio-script: FAILED (%s): %s\n", st->fail_class && *st->fail_class ? st->fail_class : st->state,
                st->summary);
    if (st->hint[0]) fprintf(f, "  what to do: %s\n", st->hint);
    fprintf(f, "  %s", strcmp(st->mode, "verify") ? "run" : "verification only");
    when(f, " started", st->started);
    if (st->finished) fprintf(f, ", took %.1f s", st->seconds);
    else fprintf(f, ", pid %d", st->pid);
    fprintf(f, "\n");
    if (st->sdk[0]) fprintf(f, "  switch chip: %s\n", st->sdk);
    for (i = 0; i < st->nscripts; i++) {
        const mdio_status_script_t* s = &st->script[i];
        if (!s->started) {
            fprintf(f, "  %s: not run (%lu operations)\n", s->path, s->parsed.ops);
            continue;
        }
        fprintf(f, "  %s: %s, %lu of %lu operations in %.1f s (%lu writes, %lu reads, %lu re-reads; %lu of the recorded "
                   "values differed)\n",
                s->path, s->result == MDIO_SCRIPT_OK ? "done" : "STOPPED", s->run.ops, s->parsed.ops, s->seconds,
                s->run.writes, s->run.reads, s->run.poll_rereads, s->run.differs);
    }
    if (st->temp_enabled)
        fprintf(f, "  ASIC temperature published meanwhile: %d readings, last %.1f C, %d failed, longest read %.2f ms\n",
                st->temp_reads, st->temp_last_c, st->temp_failed, st->temp_max_ms);
    if (st->report && st->report->nphy) {
        fprintf(f, "  PHY registers read at %s: %d of %d PHYs as expected\n", st->report_when ? st->report_when : "the end",
                mdio_report_count(st->report, MDIO_PHY_OK), st->report->nphy);
        mdio_report_table(f, st->report, st->names);
    }
}

static int write_atomic(const char* path, const mdio_status_t* st, void (*fn)(FILE*, const mdio_status_t*))
{
    char tmp[4096];
    FILE* f;
    if (snprintf(tmp, sizeof(tmp), "%s.tmp", path) >= (int)sizeof(tmp)) return -1;
    f = fopen(tmp, "w");
    if (!f && errno == ENOENT) {
        char dir[4096];
        const char* s = strrchr(path, '/');
        if (s && s != path && (size_t)(s - path) < sizeof(dir)) {
            memcpy(dir, path, (size_t)(s - path));
            dir[s - path] = 0;
            if (mkdir(dir, 0755) == 0 || errno == EEXIST) f = fopen(tmp, "w");
        }
    }
    if (!f) return -1;
    fn(f, st);
    if (fflush(f) != 0 || ferror(f)) {
        fclose(f);
        unlink(tmp);
        return -1;
    }
    fchmod(fileno(f), 0644);
    if (fclose(f) != 0 || rename(tmp, path) != 0) {
        unlink(tmp);
        return -1;
    }
    return 0;
}

int mdio_status_write(const char* json_path, const mdio_status_t* st)
{
    char txt[4096];
    size_t n = strlen(json_path);
    int rv;
    if (n > 5 && !strcmp(json_path + n - 5, ".json")) snprintf(txt, sizeof(txt), "%.*s.txt", (int)(n - 5), json_path);
    else snprintf(txt, sizeof(txt), "%s.txt", json_path);
    rv = write_atomic(json_path, st, mdio_status_json);
    if (write_atomic(txt, st, mdio_status_text) != 0) rv = -1;
    return rv;
}

void mdio_sd_status(const char* text)
{
    const char* path = getenv("NOTIFY_SOCKET");
    struct sockaddr_un a;
    char msg[900];
    int fd, n;
    size_t len;
    if (!path || !*path || (path[0] != '/' && path[0] != '@')) return;
    len = strlen(path);
    if (len >= sizeof(a.sun_path)) return;
    memset(&a, 0, sizeof(a));
    a.sun_family = AF_UNIX;
    memcpy(a.sun_path, path, len);
    if (path[0] == '@') a.sun_path[0] = 0;
    n = snprintf(msg, sizeof(msg), "STATUS=%s", text);
    if (n >= (int)sizeof(msg)) n = (int)sizeof(msg) - 1;
    fd = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return;
    (void)!sendto(fd, msg, (size_t)n, MSG_NOSIGNAL, (struct sockaddr*)&a,
                  (socklen_t)(offsetof(struct sockaddr_un, sun_path) + len));
    close(fd);
}

/* ----------------------------------------------------------------- system */

int mdio_lock_holder(const char* lock_path, int* pid, char* comm, int commlen)
{
    struct stat sb;
    char line[256];
    FILE* f;
    int found = 0;
    if (comm && commlen > 0) comm[0] = 0;
    if (stat(lock_path, &sb) != 0) return 0;
    f = fopen("/proc/locks", "r");
    if (!f) return 0;
    while (!found && fgets(line, sizeof(line), f)) {
        char type[16];
        unsigned maj, min;
        unsigned long ino;
        int p;
        /* "12: FLOCK  ADVISORY  WRITE 812 00:1a:1234 0 EOF"; waiters are "12: -> FLOCK ...". */
        if (strstr(line, "->")) continue;
        if (sscanf(line, "%*d: %15s %*s %*s %d %x:%x:%lu", type, &p, &maj, &min, &ino) != 5) continue;
        if (strcmp(type, "FLOCK") != 0 || ino != (unsigned long)sb.st_ino || maj != major(sb.st_dev) ||
            min != minor(sb.st_dev))
            continue;
        *pid = p;
        found = 1;
    }
    fclose(f);
    if (found && comm && commlen > 0) {
        snprintf(line, sizeof(line), "/proc/%d/comm", *pid);
        f = fopen(line, "r");
        if (f) {
            if (fgets(comm, commlen, f)) comm[strcspn(comm, "\n")] = 0;
            fclose(f);
        }
    }
    return found;
}

int mdio_diag_c45_read(const char* socket_path, unsigned phy, unsigned dev, unsigned reg, unsigned* val, char* err,
                       int errlen)
{
    struct sockaddr_un a;
    char cmd[80], buf[1024], *p, *end;
    size_t len = 0;
    ssize_t n;
    unsigned long v;
    int fd;
    if (dev < 1 || dev > 31 || reg > 0xffff) {
        snprintf(err, (size_t)errlen, "clause 45 only (dev 1..31)");
        return -1;
    }
    memset(&a, 0, sizeof(a));
    a.sun_family = AF_UNIX;
    if (strlen(socket_path) >= sizeof(a.sun_path)) {
        snprintf(err, (size_t)errlen, "%s: path too long", socket_path);
        return -1;
    }
    strcpy(a.sun_path, socket_path);
    fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0 || connect(fd, (struct sockaddr*)&a, sizeof(a)) != 0) {
        snprintf(err, (size_t)errlen, "%s: %s (is syncd running with SAI_OPENBCM_DIAG_SOCKET?)", socket_path,
                 strerror(errno));
        if (fd >= 0) close(fd);
        return -1;
    }
    n = snprintf(cmd, sizeof(cmd), "phy raw c45 0x%x 0x%x 0x%x\n", phy, dev, reg);
    if (write(fd, cmd, (size_t)n) != n) {
        snprintf(err, (size_t)errlen, "%s: write: %s", socket_path, strerror(errno));
        close(fd);
        return -1;
    }
    shutdown(fd, SHUT_WR);
    while (len < sizeof(buf) - 1 && (n = read(fd, buf + len, sizeof(buf) - 1 - len)) > 0) len += (size_t)n;
    close(fd);
    buf[len] = 0;
    /* The shell prints "\t0x<reg>: 0x<value>"; the server prefixes its own
     * refusals and failures with "openbcm-diag: ". */
    p = strstr(buf, "openbcm-diag: ");
    if (p) {
        p[strcspn(p, "\n")] = 0;
        snprintf(err, (size_t)errlen, "%s", p);
        return -1;
    }
    p = NULL;
    for (end = buf; (end = strstr(end, ": 0x")) != NULL; end += 4) p = end + 2;
    if (!p) {
        buf[strcspn(buf, "\n")] = 0;
        snprintf(err, (size_t)errlen, "no value in the diag shell's answer to '%.*s': '%.80s'", (int)strlen(cmd) - 1, cmd,
                 buf);
        return -1;
    }
    errno = 0;
    v = strtoul(p, &end, 16);
    if (errno || end == p + 2 || v > 0xffff) {
        snprintf(err, (size_t)errlen, "unparsable value in the diag shell's answer: '%.40s'", p);
        return -1;
    }
    *val = (unsigned)v;
    return 0;
}
