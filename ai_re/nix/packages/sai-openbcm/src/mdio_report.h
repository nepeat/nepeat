/* SPDX-License-Identifier: Apache-2.0
 * What openbcm-mdio-script tells people and programs about a run: the
 * per-PHY result of a script's verification section, the classification of
 * a failure, the status files, and the two things it needs from the system
 * for that (who holds the SDK lock, MDIO reads through syncd's diag
 * socket). No SDK code and no knowledge of any PHY: what is checked comes
 * from the script, names for PHYs and registers from a names file.
 *
 * Names file (optional), one entry per line, '#' comments:
 *   phy <address word, hex> <name>          e.g.  phy 24 cage5
 *   reg <dev>.<reg, hex> <name>             e.g.  reg 1.8215 fw
 */
#ifndef SAI_OPENBCM_MDIO_REPORT_H
#define SAI_OPENBCM_MDIO_REPORT_H

#include <stdio.h>

#include "mdio_script.h"

#ifdef __cplusplus
extern "C" {
#endif

#define MDIO_REPORT_MAX_PHYS 256
#define MDIO_REPORT_MAX_CHECKS 12

enum {
    MDIO_PHY_OK = 0,
    MDIO_PHY_MISMATCH,   /* answers, some register is not what the script expects */
    MDIO_PHY_CONSTANT,   /* every verified register reads the same value: no PHY answering, or a hung one */
    MDIO_PHY_IO          /* the bus master reported an error */
};

typedef struct {
    unsigned dev, reg, want, mask, got;
    char op;
    int ok, io_error;
} mdio_report_check_t;

typedef struct {
    unsigned phy;
    int n, state;
    unsigned constant;   /* MDIO_PHY_CONSTANT: the value */
    mdio_report_check_t c[MDIO_REPORT_MAX_CHECKS];
} mdio_report_phy_t;

typedef struct {
    int nphy, dropped;   /* dropped: checks beyond the table sizes */
    mdio_report_phy_t phy[MDIO_REPORT_MAX_PHYS];
} mdio_report_t;

typedef struct {
    int nphy, nreg;
    struct { unsigned phy; char name[24]; } p[MDIO_REPORT_MAX_PHYS];
    struct { unsigned dev, reg; char name[12]; } r[32];
} mdio_names_t;

void mdio_report_reset(mdio_report_t* r);
/* io->verify callback body: file one check under its PHY (order of first appearance). */
void mdio_report_add(mdio_report_t* r, const mdio_script_check_t* c);
/* Set every PHY's state. A PHY is CONSTANT when at least 3 registers were
 * read, all returned one value and that is not what the script expects. */
void mdio_report_classify(mdio_report_t* r);
int mdio_report_count(const mdio_report_t* r, int state);
const mdio_report_phy_t* mdio_report_find(const mdio_report_t* r, unsigned phy);
const char* mdio_phy_state_name(int state);   /* "ok" "mismatch" "constant" "io-error" */
/* 0, or -1 with err set (file unreadable, bad line). */
int mdio_names_load(const char* path, mdio_names_t* n, char* err, int errlen);
const char* mdio_names_phy(const mdio_names_t* n, unsigned phy);          /* NULL: none */
const char* mdio_names_reg(const mdio_names_t* n, unsigned dev, unsigned reg);
/* One row per PHY: name, address, the value of every verified register
 * ('!' after one that differs), state. names may be NULL. */
void mdio_report_table(FILE* f, const mdio_report_t* r, const mdio_names_t* names);

/* ---- failure classification ---- */
#define MDIO_CLASS_SCRIPT_MISSING "script-missing"       /* file not there / unreadable */
#define MDIO_CLASS_SCRIPT_MALFORMED "script-malformed"   /* truncated or not a script */
#define MDIO_CLASS_SDK_BUSY "sdk-busy"                   /* another process owns the switch chip */
#define MDIO_CLASS_SDK_CONFIG "sdk-config-missing"
#define MDIO_CLASS_SDK_INIT "sdk-init"                   /* attach / init soc / init misc failed */
#define MDIO_CLASS_MDIO_CLOCK "mdio-clock"               /* MDIO rate or hold delay on the chip not as configured */
#define MDIO_CLASS_MDIO_IO "mdio-io"                     /* the MDIO master reported an error */
#define MDIO_CLASS_PHY_CONSTANT "phy-not-answering"      /* a PHY reads one constant everywhere */
#define MDIO_CLASS_DIVERGED "sequence-diverged"          /* a poll never saw its value; the PHY answers */
#define MDIO_CLASS_VERIFY "verify-mismatch"              /* ran to the end, read-back differs */
#define MDIO_CLASS_INTERRUPTED "interrupted"
/* Class of a failed script run: rv = MDIO_SCRIPT_E_*, fail = its record,
 * post = the verification reads made at the end or after the failure
 * (classified; NULL if none could be made). */
const char* mdio_classify_run(int rv, const mdio_script_fail_t* fail, const mdio_report_t* post);
/* What to do about a class, in general terms (boards add their own). */
const char* mdio_class_hint(const char* cls);

/* ---- status files ---- */
#define MDIO_STATUS_MAX_SCRIPTS 8
typedef struct {
    const char* path;
    mdio_script_stats_t parsed, run;   /* parsed.ops = operations in the file */
    int started;                       /* run holds numbers */
    int result;                        /* MDIO_SCRIPT_* of the run */
    double seconds;
} mdio_status_script_t;

typedef struct {
    const char* state;                 /* "running" "ok" "failed" "interrupted" */
    const char* mode;                  /* "run" "verify" */
    long long started, finished;       /* unix time, finished 0 while running */
    double seconds;
    int pid;
    char sdk[320];                     /* the chip / MDIO clock line, "" before the SDK is up */
    int nscripts;
    mdio_status_script_t script[MDIO_STATUS_MAX_SCRIPTS];
    /* failure ("" / NULL when none) */
    const char* fail_class;
    const char* fail_script;
    mdio_script_fail_t fail;
    char summary[700];                 /* one line: what happened */
    char hint[400];                    /* one line: what to do */
    /* verification */
    const mdio_report_t* report;       /* NULL: no verification reads were made */
    const char* report_when;           /* "end of the script" / "after the failure" / "now" */
    const mdio_names_t* names;
    /* ASIC temperature published while the script ran */
    int temp_enabled, temp_reads, temp_failed;
    double temp_last_c, temp_max_ms;
} mdio_status_t;

/* The one-line summary of a finished run into st->summary (and the hint),
 * from the fields above: call before writing. */
void mdio_status_summarize(mdio_status_t* st);
void mdio_status_json(FILE* f, const mdio_status_t* st);
/* Line 1 is "openbcm-mdio-script: ok: ..." / "...: FAILED (class): ..." /
 * "...: running: ..."; then the details and the PHY table. */
void mdio_status_text(FILE* f, const mdio_status_t* st);
/* JSON to json_path and the text next to it (".json" -> ".txt", else
 * json_path + ".txt"), each replaced atomically; the directory is created
 * (one level). 0 or -1 (errno). */
int mdio_status_write(const char* json_path, const mdio_status_t* st);
/* "STATUS=<text>" to systemd's notify socket, when there is one. */
void mdio_sd_status(const char* text);

/* ---- system ---- */
/* The process holding an flock on lock_path (from /proc/locks): 1 with *pid
 * and its command name, 0 if nobody (or it cannot be told). */
int mdio_lock_holder(const char* lock_path, int* pid, char* comm, int commlen);
/* One clause-45 read through syncd's diag socket (sai_diag.cpp): the diag
 * shell's "phy raw c45 <phy> <dev> <reg>". 0, or -1 with err set. Root
 * only, as the socket is. */
int mdio_diag_c45_read(const char* socket_path, unsigned phy, unsigned dev, unsigned reg, unsigned* val, char* err,
                       int errlen);

#ifdef __cplusplus
}
#endif
#endif
