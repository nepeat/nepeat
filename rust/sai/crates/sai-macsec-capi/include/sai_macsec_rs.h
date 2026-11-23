/* SPDX-License-Identifier: Apache-2.0 */
/*
 * sai_macsec_rs.h: the C interface of the Rust MACsec object model (rust/sai/crates/sai-macsec)
 * for the C++ sai-openbcm (make RUST_MACSEC=...: src/sai_macsec_rust.cpp replaces
 * src/sai_macsec.cpp). Library: libsai_macsec_capi.a, which also carries the whole phy_seq.h
 * interface (one Rust static library per link: two would each bring Rust's std).
 *
 * This header is the interface: the Rust side generates its declarations from it (bindgen)
 * and its functions are type-checked against them.
 *
 * Division of work
 *  - C++ keeps: the SAI method tables and guarded() (the library lock, the "switch created"
 *    check), reading and parsing the table file (ext_macsec_load, either engine), running a
 *    sequence (ext_macsec_exec under the external PHY table's sequence lock, MDIO through the
 *    backend, the "extmacsec trace"), the inter-frame gap (port_ifg_apply), object id
 *    allocation (ctx().alloc, shared with every other object), logging (OB_LOG), the parameter
 *    file of "extmacsec run".
 *  - Rust keeps: every MACsec object and MACsec-stage ACL object, every check and status, which
 *    sequence runs when with which parameter bytes, the counters, the status text.
 *
 * Rules for the caller
 *  - Every SAI-shaped function returns a sai_status_t and never unwinds into the caller (a
 *    defect inside is contained and reported as SAI_STATUS_FAILURE; after a panic every call
 *    fails until ob_rs_macsec_detach).
 *  - Call with the library lock held (the C++ does: guarded, or its callers' guarded).
 *  - Pointers: as SAI's. A count of 0 needs no pointer; a null pointer where one is needed is
 *    an error (SAI_STATUS_INVALID_PARAMETER) where the C++ model checked it, and where it did
 *    not. The caller guarantees what cannot be checked: a non-null pointer points to as many
 *    initialised elements as its count says (output: writable), the union member SAI defines
 *    for an attribute is the one initialised, nothing else accesses them during the call.
 *  - Callbacks are called on the calling thread, during the call that triggers them, and must
 *    return normally (no C++ exception, no longjmp) and must not call back into this library
 *    (a nested call fails with SAI_STATUS_FAILURE instead of deadlocking).
 *  - Key material: SAK, salt and hash key are copied from the attribute straight into storage
 *    that is wiped when it is no longer needed (once installed, or when the SA goes); the
 *    ob_rs_param bytes handed to run() are lent for that call only. No log line, status text
 *    or message holds a key byte.
 *  - 64-bit targets only.
 */
#ifndef SAI_MACSEC_RS_H
#define SAI_MACSEC_RS_H

#include <stddef.h>
#include <stdint.h>
#include <sai.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The version of this interface; ob_rs_macsec_abi() returns the library's. */
#define OB_RS_MACSEC_ABI 1u

/* One parameter of a sequence run (key material: lent for the call). */
typedef struct ob_rs_param {
    const char* name; /* "key", "hkey", "sci", "salt", "ssci", "iv", "pn", "window"; not NUL-terminated */
    size_t name_len;
    const uint8_t* bytes;
    size_t len;
} ob_rs_param;

/* Receives word "word" of result "name" of a run that completed. */
typedef void (*ob_rs_put_fn)(void* ctx, const char* name, size_t name_len, uint32_t word, uint16_t value);

/* Receives a text (not NUL-terminated). */
typedef void (*ob_rs_text_fn)(void* ctx, const char* text, size_t len);

/* What the C++ does for the model. No context pointer: they work on the library's state. */
typedef struct ob_rs_callbacks {
    /* api: SAI_API_MACSEC, SAI_API_SWITCH or SAI_API_ACL (the C++ texts' APIs); level: sai_log_level_t */
    void (*log)(int32_t api, int32_t level, const char* text, size_t len);
    /* Runs sequence name on the PHY of chip port hw (ext_macsec_exec, sequence lock, trace).
     * SAI_STATUS_SUCCESS: every result word went to put(put_ctx, ...). Else the reason, NUL-terminated,
     * in why (cut to why_cap - 1 bytes; no parameter byte in it). */
    sai_status_t (*run)(uint32_t hw, const char* name, size_t name_len, const ob_rs_param* params, size_t n_params,
                        ob_rs_put_fn put, void* put_ctx, char* why, size_t why_cap);
    /* port_ifg_apply of chip port hw with the MACsec gap macsec_bits (0: the port has no MACSEC_PORT any more). */
    sai_status_t (*set_ifg)(uint32_t hw, uint32_t macsec_bits);
    /* ctx().alloc(object_type): a new id of that type; 0 = none. */
    sai_object_id_t (*alloc)(int32_t object_type);
} ob_rs_callbacks;

/* A "port <lane> <phy>" line of the table. */
typedef struct ob_rs_engine {
    uint32_t lane;
    uint32_t phy;
} ob_rs_engine;

/* A sequence of the table: its name and the result names it reads ("X ... name[i]"),
 * separated by '\n' (results may be empty). */
typedef struct ob_rs_seq {
    const char* name;
    size_t name_len;
    const char* results;
    size_t results_len;
} ob_rs_seq;

/* A port of the chip (the backend's PortInfo). */
typedef struct ob_rs_chip_port {
    uint32_t hw;
    uint32_t has_lane;   /* 0: the port has no lanes */
    uint32_t first_lane;
    const char* name;    /* the SDK's name, not NUL-terminated */
    size_t name_len;
} ob_rs_chip_port;

/* A front-panel SAI port (not the CPU port). */
typedef struct ob_rs_switch_port {
    sai_object_id_t oid;
    uint32_t hw;
} ob_rs_switch_port;

uint32_t ob_rs_macsec_abi(void);

/* ext_macsec_attach. loaded 0: the switch has no table (every create: NOT_SUPPORTED). Runs the
 * probes (callbacks->run). Replaces (wipes) a previous model. */
sai_status_t ob_rs_macsec_attach(const ob_rs_callbacks* callbacks, uint32_t loaded, uint32_t clear_on_read,
                                 uint32_t ifg_bits, const ob_rs_engine* engines, size_t n_engines, const ob_rs_seq* seqs,
                                 size_t n_seqs, const ob_rs_chip_port* ports, size_t n_ports);
/* ext_macsec_detach: forgets everything (keys wiped), runs nothing; clears a contained panic. */
void ob_rs_macsec_detach(void);
/* The switch's front-panel ports: call whenever they differ from what was handed over last. */
void ob_rs_macsec_ports(const ob_rs_switch_port* ports, size_t n);

/* The objects: MACSEC, MACSEC_PORT, MACSEC_FLOW, MACSEC_SC, MACSEC_SA, and ACL_TABLE / ACL_ENTRY
 * of the MACsec stages. A create of another type: SAI_STATUS_INVALID_OBJECT_TYPE. */
sai_status_t ob_rs_macsec_create(sai_object_type_t type, sai_object_id_t* id, uint32_t n, const sai_attribute_t* attrs);
sai_status_t ob_rs_macsec_remove(sai_object_type_t type, sai_object_id_t id);
sai_status_t ob_rs_macsec_set(sai_object_type_t type, sai_object_id_t id, const sai_attribute_t* attr);
sai_status_t ob_rs_macsec_get(sai_object_type_t type, sai_object_id_t id, uint32_t n, sai_attribute_t* attrs);
/* get_*_stats (the plain read; *_stats_ext's mode check stays with the caller). */
sai_status_t ob_rs_macsec_stats(sai_object_type_t type, sai_object_id_t id, uint32_t n, const sai_stat_id_t* ids,
                                uint64_t* out);
sai_status_t ob_rs_macsec_clear_stats(sai_object_type_t type, sai_object_id_t id);
/* 1: id is an ACL table (type ACL_TABLE) / entry (ACL_ENTRY) of the MACsec stages. */
uint32_t ob_rs_macsec_owns(sai_object_type_t type, sai_object_id_t id);

/* PORT_ATTR_{INGRESS,EGRESS}_MACSEC_ACL set on port (table may be null); an attribute error is
 * for attribute index "index" of the caller's call. */
sai_status_t ob_rs_macsec_port_acl_set(sai_object_id_t port, uint32_t ingress, sai_object_id_t table, uint32_t index);
sai_object_id_t ob_rs_macsec_port_acl(sai_object_id_t port, uint32_t ingress);
/* PORT_ATTR_MACSEC_PORT_LIST (SAI's list convention). */
sai_status_t ob_rs_macsec_port_list(sai_object_id_t port, sai_object_list_t* list);
/* macsec_port_ifg. */
uint32_t ob_rs_macsec_port_ifg(uint32_t hw);
/* sai_query_stats_capability for MACSEC_SA / MACSEC_FLOW (SAI's list convention). */
sai_status_t ob_rs_macsec_stats_capability(sai_object_type_t type, sai_stat_capability_list_t* out);
/* The text of "openbcm-diag extmacsec". */
void ob_rs_macsec_status(ob_rs_text_fn out, void* ctx);
/* "extmacsec run <port> <sequence>" with the parameters of the file (the caller read and checked it). */
void ob_rs_macsec_diag_run(const char* port, size_t port_len, const char* seq, size_t seq_len, const ob_rs_param* params,
                           size_t n_params, ob_rs_text_fn out, void* ctx);

#ifdef __cplusplus
}
#endif

#endif /* SAI_MACSEC_RS_H */
