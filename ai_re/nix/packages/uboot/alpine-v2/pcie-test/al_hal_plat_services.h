/*
 * Host test shim: every register access goes through the register model in
 * pcie-init-test.c, which logs writes and simulates the LTSSM.
 */
#ifndef __PLAT_SERVICES_H__
#define __PLAT_SERVICES_H__
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include "al_hal_plat_types.h"

uint8_t  model_r8(const void *a);
uint16_t model_r16(const void *a);
uint32_t model_r32(const void *a);
void model_w8(void *a, uint8_t v);
void model_w16(void *a, uint16_t v);
void model_w32(void *a, uint32_t v);
void model_udelay(unsigned long us);
extern int al_pcie_hal_failed;

#define al_reg_read8(l)		model_r8(l)
#define al_reg_read16(l)	model_r16(l)
#define al_reg_read32(l)	model_r32(l)
#define al_reg_write8(l, v)	model_w8(l, v)
#define al_reg_write16(l, v)	model_w16(l, v)
#define al_reg_write32(l, v)	model_w32(l, v)
#define al_reg_read32_relaxed(l)	model_r32(l)
#define al_reg_write32_relaxed(l, v)	model_w32(l, v)

#define al_print(...)		printf(__VA_ARGS__)
#define al_err(...)		printf(__VA_ARGS__)
#define al_warn(...)		printf(__VA_ARGS__)
#define al_info(...)		do { } while (0)
#define al_dbg(...)		do { } while (0)
#define al_sprintf(...)		sprintf(__VA_ARGS__)
#define al_assert(COND) do { if (!(COND)) { \
	printf("HAL assertion '%s' failed at %s:%d\n", #COND, __func__, __LINE__); \
	al_pcie_hal_failed = 1; } } while (0)
#define al_assert_msg(COND, ...) do { if (!(COND)) { printf(__VA_ARGS__); al_assert(COND); } } while (0)

static inline void al_data_memory_barrier(void) { }
static inline void al_local_data_memory_barrier(void) { }
static inline void al_smp_data_memory_barrier(void) { }
static inline void al_smp_write_data_memory_barrier(void) { }
#define al_udelay(u)		model_udelay(u)
#define al_msleep(m)		model_udelay((m) * 1000)
#define swap16_to_le(x) (x)
#define swap32_to_le(x) (x)
#define swap64_to_le(x) (x)
#define swap16_from_le(x) (x)
#define swap32_from_le(x) (x)
#define swap64_from_le(x) (x)
#define al_memset(p, v, c)	memset(p, v, c)
#define al_memcpy(d, s, c)	memcpy(d, s, c)
#define al_memcmp(a, b, c)	memcmp(a, b, c)
#define al_strcmp(a, b)		strcmp(a, b)
#define al_get_cpu_id()		0
#define al_get_cluster_id()	0
#endif
