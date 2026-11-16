/* SPDX-License-Identifier: Apache-2.0
 * Mock libifcs.so for the sai-teralynx tests, one file per backend area
 * (mock_<area>.c, like src/tl_<area>.cpp). Together they export every
 * symbol the backend dlsyms (IFCS_SYMBOLS in src/ifcs_api.h): each file
 * includes this header, so a definition whose type differs from the
 * declared one does not compile, and mock_core.c references them all, so a
 * missing one does not link (-Wl,--no-undefined). Calls are recorded as
 * text lines (mock_logf); test hooks (mock_*) are reached with dlsym on the
 * same handle. Nothing here models the real SDK beyond what the tests assert. */
#ifndef SAI_TERALYNX_MOCK_H
#define SAI_TERALYNX_MOCK_H

#include "ifcs_api.h"

#include <pthread.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_DP 260
#define MAX_OBJ 4096

#define MOCK_PROTO(n) extern __typeof__(*(n##_fn)0) n;
IFCS_SYMBOLS(MOCK_PROTO)
#undef MOCK_PROTO

void mock_logf(const char* fmt, ...) __attribute__((format(printf, 1, 2)));
ifcs_status_t mock_create_obj(const char* what, ifcs_handle_t* h, uint32_t c);
void mock_port_reset(void);            /* ifcs_init: every devport admin up, 400G, KP */

extern int g_inited, g_node, g_enabled;
extern uint32_t next_handle;

static inline ifcs_handle_t sp_of(uint32_t d) { return d == 0 ? IFCS_HANDLE_SYSPORT(1000) : IFCS_HANDLE_SYSPORT(d); }
static inline int bad_dp(ifcs_devport_t d) { return d >= MAX_DP; }

#endif
