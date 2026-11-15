/* Host test shim (not the U-Boot one): plain C types. */
#ifndef __PLAT_TYPES_H__
#define __PLAT_TYPES_H__
#include <inttypes.h>
#include <stdint.h>
#include <stddef.h>
typedef int al_bool;
#define AL_TRUE			1
#define AL_FALSE		0
typedef uint64_t al_phys_addr_t;
#define PLAT_ARCH_IS_LITTLE()	AL_TRUE
#define __iomem
#endif
