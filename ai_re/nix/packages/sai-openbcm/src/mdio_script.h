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
 * P with <tries> 1 is an assertion (used for the read-back verification at
 * the end of a script). */
#ifndef SAI_OPENBCM_MDIO_SCRIPT_H
#define SAI_OPENBCM_MDIO_SCRIPT_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    /* 0 = success; anything else stops the script. */
    int (*read)(void* ctx, unsigned phy, unsigned dev, unsigned reg, unsigned* val);
    int (*write)(void* ctx, unsigned phy, unsigned dev, unsigned reg, unsigned val);
    void (*sleep_us)(void* ctx, unsigned long us);   /* NULL: nanosleep */
    void (*log)(void* ctx, const char* msg);         /* NULL: silent */
    void* ctx;
} mdio_script_io_t;

typedef struct {
    unsigned long lines, writes, reads, delays, polls, labels;
    unsigned long poll_rereads;      /* reads beyond the first of a P/N */
    unsigned long differs;           /* E mismatches */
    unsigned long long delay_us;     /* requested by D */
} mdio_script_stats_t;

#define MDIO_SCRIPT_OK 0
#define MDIO_SCRIPT_E_FILE (-1)      /* cannot open / read */
#define MDIO_SCRIPT_E_SYNTAX (-2)    /* bad line (nothing after it is run) */
#define MDIO_SCRIPT_E_POLL (-3)      /* a P/N never matched: the chip does not follow the script */
#define MDIO_SCRIPT_E_IO (-4)        /* read/write callback failed */

/* io == NULL: parse only (every line is checked, nothing is accessed). Run a
 * script only after a parse-only pass succeeded: a truncated file must not
 * leave a chip half-programmed. err receives one line naming the script
 * line, the label in force and the registers involved. */
int mdio_script_run(const char* path, const mdio_script_io_t* io, mdio_script_stats_t* st, char* err, int errlen);

#ifdef __cplusplus
}
#endif
#endif
