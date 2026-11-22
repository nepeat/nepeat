/* SPDX-License-Identifier: Apache-2.0
 * MDIO script: a list of clause-45 writes, delays, reads and polls for PHYs
 * the SDK has no driver for (retimers, MACsec PHYs with a vendor-only
 * library). The script is a recording made elsewhere; this code knows no
 * chip. Text, one operation per line, numbers hex unless noted:
 *
 *   mdio-script 1                                  first line
 *   # comment
 *   L <text>                                       label: names the step in messages
 *   D <usec, decimal>                              delay
 *   W <phy> <dev> <reg> <val>                      write
 *   R <phy> <dev> <reg>                            read, value ignored (clear-on-read, FIFO pop)
 *   E <phy> <dev> <reg> <val> [<mask>]             read; (v & mask) != val is counted and the first few logged
 *   P <phy> <dev> <reg> <val> <mask> <tries> <interval>
 *                                                  read until (v & mask) == val; the script FAILS after <tries>
 *                                                  reads (decimal), <interval> usec (decimal) apart
 *   N <phy> <dev> <reg> <val> <mask> <tries> <interval>
 *                                                  the same, until (v & mask) != val
 *
 * <phy> is the bus master's address word (OpenBCM: bus in bits 8, 6:5,
 * address in bits 4:0). <dev> is 1..31: clause 22 cannot be expressed.
 * P / N with <tries> 1 is an assertion.
 *
 * Verification section: the assertions at the very end of a script (after
 * the last W, D, R, E and multi-try P / N; L lines and comments between them
 * belong to it) are its read-back verification, typically a few identity and
 * state registers per PHY. A parse pass reports where it starts
 * (stats.verify_line). When the caller passes that line back
 * (mdio_script_opts_t.verify_line), a mismatch there does not stop the
 * script: every assertion is read and handed to io->verify, and the script
 * fails at the end with MDIO_SCRIPT_E_VERIFY, so the caller knows the state
 * of every PHY, not of the first bad one. With verify_only nothing before
 * the section runs: reads only, for a status display or a post-mortem. The
 * script's author keeps registers with read side effects out of it. */
#ifndef SAI_OPENBCM_MDIO_SCRIPT_H
#define SAI_OPENBCM_MDIO_SCRIPT_H

#ifdef __cplusplus
extern "C" {
#endif

/* One assertion of the verification section, as read. */
typedef struct mdio_script_check {
    unsigned long line;
    char op;                         /* 'P': want (got & mask) == want; 'N': != */
    unsigned phy, dev, reg, want, mask, got;
    int ok;
    int io_error;                    /* the read itself failed (got is 0) */
    const char* label;               /* label in force */
} mdio_script_check_t;

typedef struct {
    /* 0 = success; anything else stops the script. */
    int (*read)(void* ctx, unsigned phy, unsigned dev, unsigned reg, unsigned* val);
    int (*write)(void* ctx, unsigned phy, unsigned dev, unsigned reg, unsigned val);
    void (*sleep_us)(void* ctx, unsigned long us);   /* NULL: nanosleep */
    void (*log)(void* ctx, const char* msg);         /* NULL: silent */
    void* ctx;
    /* Called from the running thread before every operation, between the
     * reads of a poll and at least every 0.5 s during a delay (a place to
     * publish progress or do periodic work on the same bus master without a
     * second thread). Non-zero stops the script: MDIO_SCRIPT_E_ABORT. NULL: none. */
    int (*tick)(void* ctx);
    /* Every assertion of the verification section, with the value read. NULL: none. */
    void (*verify)(void* ctx, const struct mdio_script_check* c);
} mdio_script_io_t;

typedef struct {
    unsigned long lines, writes, reads, delays, polls, labels;
    unsigned long poll_rereads;      /* reads beyond the first of a P/N */
    unsigned long differs;           /* E mismatches */
    unsigned long long delay_us;     /* requested by D */
    unsigned long ops;               /* W R E P N D lines seen (parse pass) or completed (run) */
    unsigned long verify_line;       /* first line of the verification section, 0 = none */
    unsigned long verify_checks;     /* assertions in it (parse pass) or read (run) */
    unsigned long verify_failed;     /* of those, mismatches and read errors */
} mdio_script_stats_t;

/* Where and why a run stopped (all zero when it did not). */
typedef struct {
    unsigned long line;              /* script line, 0 = not about a line */
    char op;                         /* W R E P N, 0 = none */
    unsigned phy, dev, reg, want, mask, got;
    unsigned long tries;             /* reads made by a P / N */
    int have_value;                  /* got was read from the bus */
    char label[160];                 /* label in force */
} mdio_script_fail_t;

typedef struct {
    unsigned long verify_line;       /* from a parse pass; 0 = assertions at the end stop the script like any other */
    int verify_only;                 /* run the verification section only (needs verify_line) */
    mdio_script_fail_t* fail;        /* may be NULL */
} mdio_script_opts_t;

#define MDIO_SCRIPT_OK 0
#define MDIO_SCRIPT_E_FILE (-1)      /* cannot open / read */
#define MDIO_SCRIPT_E_SYNTAX (-2)    /* bad line (nothing after it is run) */
#define MDIO_SCRIPT_E_POLL (-3)      /* a P/N never matched: the chip does not follow the script */
#define MDIO_SCRIPT_E_IO (-4)        /* read/write callback failed */
#define MDIO_SCRIPT_E_VERIFY (-5)    /* the script ran to its end; verification assertions failed */
#define MDIO_SCRIPT_E_ABORT (-6)     /* io->tick asked to stop */

/* io == NULL: parse only (every line is checked, nothing is accessed). Run a
 * script only after a parse-only pass succeeded: a truncated file must not
 * leave a chip half-programmed. err receives one line naming the script
 * line, the label in force and the registers involved. */
int mdio_script_run(const char* path, const mdio_script_io_t* io, mdio_script_stats_t* st, char* err, int errlen);
/* The same with options (NULL = none). st is updated while the script runs,
 * so io->tick may read it for progress. */
int mdio_script_run_opts(const char* path, const mdio_script_io_t* io, const mdio_script_opts_t* opts,
                         mdio_script_stats_t* st, char* err, int errlen);

#ifdef __cplusplus
}
#endif
#endif
