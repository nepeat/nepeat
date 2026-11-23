/* Host shim for the difftest: al_hal_plat_types.h (as ../../uboot/alpine-v2/eth-test). */
#ifndef __PLAT_TYPES_H__
#define __PLAT_TYPES_H__
#include <stdint.h>
#include <inttypes.h>
#include <stddef.h>
typedef int al_bool;
#define AL_TRUE 1
#define AL_FALSE 0
typedef uint64_t dma_addr_t;
typedef uint64_t al_phys_addr_t;
#define PLAT_ARCH_IS_LITTLE() AL_TRUE
#endif
