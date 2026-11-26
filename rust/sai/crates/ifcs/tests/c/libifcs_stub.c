/* SPDX-License-Identifier: Apache-2.0
 * A stand-in libifcs for the loader test (tests/load.rs): every IFCS_SYMBOLS entry of the
 * generated header defined (as a stub nobody calls: only dlsym sees them); with -DOMIT_DEINIT
 * ifcs_deinit is renamed, so it is missing. */
#include "ifcs_api.h"

#ifdef OMIT_DEINIT
#define ifcs_deinit ifcs_deinit_omitted
#endif
#define STUB(n) int n(void); int n(void) { return 0; }
IFCS_SYMBOLS(STUB)
