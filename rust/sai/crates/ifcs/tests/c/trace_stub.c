/* SPDX-License-Identifier: Apache-2.0
 * A stand-in call tracer for the loader test (tests/load.rs): ifcs_trace_bind records the handle
 * it is given (-DNO_BIND: none), ifcs_deinit is "traced" (answers 40 + its argument), every other
 * function comes from libifcs. */
#include <stdint.h>

static void* bound;
#ifndef NO_BIND
void ifcs_trace_bind(void* h);
void ifcs_trace_bind(void* h) { bound = h; }
#endif
void* trace_stub_bound(void);
void* trace_stub_bound(void) { return bound; }
int ifcs_deinit(int t);
int ifcs_deinit(int t) { return 40 + t; }
