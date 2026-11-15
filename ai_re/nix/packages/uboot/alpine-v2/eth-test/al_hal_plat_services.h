/* Host shim for unit tests: registers are plain memory. */
#ifndef __PLAT_SERVICES_H__
#define __PLAT_SERVICES_H__
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#define __iomem
#define likely(x) (x)
#define unlikely(x) (x)
#define al_reg_read32(l) (*(volatile uint32_t *)(l))
#define al_reg_write32(l, v) (*(volatile uint32_t *)(l) = (v))
#define al_reg_read16(l) (*(volatile uint16_t *)(l))
#define al_reg_write16(l, v) (*(volatile uint16_t *)(l) = (v))
#define al_reg_read8(l) (*(volatile uint8_t *)(l))
#define al_reg_write8(l, v) (*(volatile uint8_t *)(l) = (v))
#define al_reg_read64(l) (*(volatile uint64_t *)(l))
#define al_reg_write64(l, v) (*(volatile uint64_t *)(l) = (v))
#define al_reg_read32_relaxed al_reg_read32
#define al_reg_write32_relaxed al_reg_write32
#define al_print(...) printf(__VA_ARGS__)
#define al_err(...) printf(__VA_ARGS__)
#define al_warn(...) printf(__VA_ARGS__)
#define al_info(...) do { } while (0)
#define al_dbg(...) do { } while (0)
#define al_assert(c) assert(c)
static inline void al_data_memory_barrier(void) { }
static inline void al_smp_data_memory_barrier(void) { }
static inline void al_local_data_memory_barrier(void) { }
#define al_udelay(u) do { } while (0)
#define al_msleep(m) do { } while (0)
#define swap16_to_le(x) (x)
#define swap32_to_le(x) (x)
#define swap64_to_le(x) (x)
#define swap16_from_le(x) (x)
#define swap32_from_le(x) (x)
#define swap64_from_le(x) (x)
#define al_memset memset
#define al_memcmp memcmp
#define al_memcpy memcpy
#define al_popcount(x) __builtin_popcount(x)
#endif
