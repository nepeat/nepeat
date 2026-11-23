/* SPDX-License-Identifier: Apache-2.0 */
/*
 * phy_seq.h: the C interface of the Rust crate phy-seq (rust/sai/crates/phy-seq): the parser
 * and the sequence executor of the external PHY tables "ext-macsec 1" and "ext-phy 1"
 * (formats: sai-openbcm's ext_macsec.h and ext_phy.h). Library: libphy_seq_capi.a.
 *
 * This header is the interface: the Rust side generates its declarations from it at build
 * time (bindgen) and its functions are type-checked against them, so the two cannot drift.
 *
 * Rules for the caller
 *  - Every function returns PHY_SEQ_OK or a PHY_SEQ_E_* code and never unwinds into the
 *    caller: a defect inside the library is contained and reported as PHY_SEQ_E_PANIC.
 *  - Pointers: an array argument is a pointer plus a count; a count of 0 needs no pointer.
 *    A null or misaligned pointer where one is needed gives PHY_SEQ_E_ARG. What the library
 *    cannot check and the caller guarantees: a non-null pointer points to as many initialised
 *    elements as the count says (output pointers: to writable storage), and nothing writes
 *    them during the call.
 *  - Callbacks (phy_seq_io) are called on the calling thread, during phy_seq_exec only.
 *    They must return normally: no C++ exception, no longjmp through the library.
 *  - A table handle is immutable: it may be used from several threads at once. Everything
 *    phy_seq_table_seq returns points into the table and lives until phy_seq_free.
 *  - Key material: the bytes of phy_seq_param are copied into storage that is wiped before
 *    phy_seq_exec returns, are never logged and are in no message. The words written to the
 *    bus callbacks are made from them.
 *  - 64-bit targets only (the layout assertions below).
 */
#ifndef PHY_SEQ_H
#define PHY_SEQ_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The version of this interface; phy_seq_abi() returns the library's. */
#define PHY_SEQ_ABI 1u

/* Return codes. */
#define PHY_SEQ_OK 0
#define PHY_SEQ_E_PARSE 1 /* the text is not a table; the message ("line N: ...") is in err */
#define PHY_SEQ_E_ARG 2   /* a null pointer, an index out of range, an unknown format, an operation no table could hold */
#define PHY_SEQ_E_PARAM 3 /* a parameter byte the sequence names is missing: nothing was accessed */
#define PHY_SEQ_E_BUS 4   /* a read / write callback failed: its status is in phy_seq_outcome.bus_status */
#define PHY_SEQ_E_FAULT 5 /* an E read another value, or a P never read its value */
#define PHY_SEQ_E_PANIC 6 /* a defect inside the library (contained) */

/* Table formats. */
#define PHY_SEQ_FORMAT_EXT_MACSEC 0u
#define PHY_SEQ_FORMAT_EXT_PHY 1u

/* phy_seq_op.value_kind (W). */
#define PHY_SEQ_VALUE_CONST 0 /* .value */
#define PHY_SEQ_VALUE_BYTES 1 /* high byte .hi_*, low byte .lo_* */
#define PHY_SEQ_VALUE_KEPT 2  /* ($<.kept> & .mask) | .bits */

/* phy_seq_op.dest_kind (X). */
#define PHY_SEQ_DEST_KEPT 0   /* $<.kept> */
#define PHY_SEQ_DEST_RESULT 1 /* word .word of the result named names[.name_off .. .name_off + .name_len] */

/* A byte of a written word: .hi_param / .lo_param is PHY_SEQ_PARAM_NONE (the byte is the
 * constant .hi_value / .lo_value) or a parameter (then .hi_value / .lo_value is the index
 * of its byte). Also phy_seq_param.param. */
#define PHY_SEQ_PARAM_NONE 0
#define PHY_SEQ_PARAM_KEY 1
#define PHY_SEQ_PARAM_HKEY 2
#define PHY_SEQ_PARAM_SCI 3
#define PHY_SEQ_PARAM_SALT 4
#define PHY_SEQ_PARAM_SSCI 5
#define PHY_SEQ_PARAM_IV 6
#define PHY_SEQ_PARAM_PN 7
#define PHY_SEQ_PARAM_WINDOW 8

/* One operation of a sequence. No pointers: the same layout on every target. */
typedef struct phy_seq_op {
    uint32_t line;     /* table line, for messages */
    uint32_t tries;    /* P: reads at most, 1..1000 */
    uint32_t usec;     /* D: the delay; P: the interval; 0..100000 */
    uint32_t name_off; /* X to a result: where its name starts in the names of the sequence */
    uint32_t name_len;
    uint16_t phy;      /* the MDIO address word, when !port_phy */
    uint16_t reg;
    uint16_t value;    /* W constant; E P C: the value compared with */
    uint16_t mask;     /* E P C: the bits compared; W kept: AND */
    uint16_t bits;     /* W kept: OR */
    uint8_t op;        /* 'W' 'R' 'E' 'P' 'X' 'D' 'C' */
    uint8_t port_phy;  /* 1: the PHY the sequence runs for */
    uint8_t dev;       /* 1..31 */
    uint8_t value_kind;
    uint8_t dest_kind;
    uint8_t kept;      /* 0..15 */
    uint8_t word;      /* 0..7 */
    uint8_t hi_param;
    uint8_t hi_value;
    uint8_t lo_param;
    uint8_t lo_value;
    uint8_t reserved[3]; /* 0 */
} phy_seq_op;

/* A parsed table. */
typedef struct phy_seq_table phy_seq_table;

typedef struct phy_seq_info {
    uint32_t format;        /* PHY_SEQ_FORMAT_* */
    uint32_t ports;
    uint32_t sequences;     /* ext-macsec: the named sequences, in name order;
                               ext-phy: 3 per port (3 * port + 0 link, + 1 admin-down, + 2 admin-up) */
    uint32_t clear_on_read; /* ext-macsec "counters": 1 clear-on-read, 0 absolute */
    uint32_t ifg_bits;      /* ext-macsec */
    uint32_t poll_ms;       /* ext-phy */
    uint32_t up_polls;      /* ext-phy */
    uint32_t holdoff_ms;    /* ext-phy */
} phy_seq_info;

typedef struct phy_seq_port {
    uint32_t lane; /* the port's first lane */
    uint32_t phy;  /* ext-macsec: the PHY's MDIO address word */
    uint32_t line; /* ext-phy: the table line of the port */
} phy_seq_port;

typedef struct phy_seq_seq {
    const char* name; /* ext-macsec: the sequence's name; ext-phy: "link", "admin-down", "admin-up". Not NUL-terminated */
    size_t name_len;
    const phy_seq_op* ops;
    size_t n_ops;
    const char* names; /* the result names the operations refer to */
    size_t names_len;
} phy_seq_seq;

/* One parameter of a run. */
typedef struct phy_seq_param {
    const uint8_t* bytes;
    size_t len;
    uint8_t param; /* PHY_SEQ_PARAM_KEY .. PHY_SEQ_PARAM_WINDOW */
} phy_seq_param;

/* The bus master and the clock of a run. op is the index of the operation the access belongs
 * to. read and write return 0 or the caller's own non-zero status. */
typedef struct phy_seq_io {
    void* ctx;
    int32_t (*read)(void* ctx, uint32_t op, uint16_t phy, uint8_t dev, uint16_t reg, uint16_t* value);
    int32_t (*write)(void* ctx, uint32_t op, uint16_t phy, uint8_t dev, uint16_t reg, uint16_t value);
    void (*delay)(void* ctx, uint32_t op, uint32_t usec);
    /* Null, or called after a run that reached its end, once per word of every named result. */
    void (*result)(void* ctx, const char* name, size_t name_len, uint32_t word, uint16_t value);
} phy_seq_io;

typedef struct phy_seq_outcome {
    int32_t bus_status; /* PHY_SEQ_E_BUS: what the callback returned; else 0 */
    uint32_t cond;      /* PHY_SEQ_OK: 1 when every C condition held (1 without any) */
    uint32_t last;      /* PHY_SEQ_OK: the value of the last C read (0 without any) */
} phy_seq_outcome;

/* PHY_SEQ_ABI of the library. */
uint32_t phy_seq_abi(void);

/* Parses a table of the given format. PHY_SEQ_OK: *table is the table (phy_seq_free it).
 * PHY_SEQ_E_PARSE: err holds the parser's message, NUL-terminated, cut to err_cap - 1 bytes
 * and at a NUL byte of the table that a message would repeat. *table is null on any error. */
int32_t phy_seq_parse(uint32_t format, const uint8_t* text, size_t len, phy_seq_table** table, char* err,
                      size_t err_cap);

/* Frees a table; null is allowed. The handle and what phy_seq_table_seq returned are dead after. */
void phy_seq_free(phy_seq_table* table);

int32_t phy_seq_table_info(const phy_seq_table* table, phy_seq_info* info);
/* Port index (0 .. info.ports - 1; ext-macsec: by lane, ext-phy: in table order). */
int32_t phy_seq_table_port(const phy_seq_table* table, uint32_t index, phy_seq_port* port);
/* Sequence index (0 .. info.sequences - 1). An ext-phy section a port does not have has n_ops 0. */
int32_t phy_seq_table_seq(const phy_seq_table* table, uint32_t index, phy_seq_seq* seq);

/* Runs operations in order for the port whose PHY has the address word phy.
 * format selects which operations are allowed (X D: ext-macsec; C: ext-phy) and the wording of
 * the message. The operations are checked first (PHY_SEQ_E_ARG: nothing was accessed), then
 * the parameters (PHY_SEQ_E_PARAM: nothing was accessed). On any error why holds a message,
 * NUL-terminated, cut to why_cap - 1 bytes; it contains no parameter byte. outcome may be null. */
int32_t phy_seq_exec(uint32_t format, const phy_seq_op* ops, size_t n_ops, const char* names, size_t names_len,
                     uint16_t phy, const phy_seq_param* params, size_t n_params, const phy_seq_io* io,
                     phy_seq_outcome* outcome, char* why, size_t why_cap);

#ifdef __cplusplus
}
#define PHY_SEQ_STATIC_ASSERT(c) static_assert(c, #c)
#else
#define PHY_SEQ_STATIC_ASSERT(c) _Static_assert(c, #c)
#endif
PHY_SEQ_STATIC_ASSERT(sizeof(phy_seq_op) == 44);
PHY_SEQ_STATIC_ASSERT(sizeof(void*) == 8 && sizeof(size_t) == 8);
PHY_SEQ_STATIC_ASSERT(sizeof(phy_seq_seq) == 48);
PHY_SEQ_STATIC_ASSERT(sizeof(phy_seq_info) == 32 && sizeof(phy_seq_port) == 12 && sizeof(phy_seq_outcome) == 12);
#undef PHY_SEQ_STATIC_ASSERT

#endif /* PHY_SEQ_H */
