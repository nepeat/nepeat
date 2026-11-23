/*
 * Host shim for the difftest: every register access of the vendor HAL goes to
 * mock.c, which keeps a register bank per BAR and records the access sequence.
 */
#ifndef __PLAT_SERVICES_H__
#define __PLAT_SERVICES_H__
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include "al_hal_plat_types.h"
#define __iomem
#define likely(x) (x)
#define unlikely(x) (x)
uint32_t mock_r32(const volatile void *p);
void mock_w32(volatile void *p, uint32_t v);
uint16_t mock_r16(const volatile void *p);
void mock_w16(volatile void *p, uint16_t v);
void mock_udelay(unsigned int us);
void mock_mb(void);
#define al_reg_read32(l) mock_r32((const volatile void *)(l))
#define al_reg_write32(l, v) mock_w32((volatile void *)(l), (v))
#define al_reg_read16(l) mock_r16((const volatile void *)(l))
#define al_reg_write16(l, v) mock_w16((volatile void *)(l), (v))
/* 8/64-bit accesses: not on the paths under test; mock.c aborts if one happens */
uint8_t mock_r8(const volatile void *p);
void mock_w8(volatile void *p, uint8_t v);
#define al_reg_read8(l) mock_r8((const volatile void *)(l))
#define al_reg_write8(l, v) mock_w8((volatile void *)(l), (v))
#define al_reg_read32_relaxed al_reg_read32
#define al_reg_write32_relaxed al_reg_write32
#define al_print(...) do { } while (0)
#define al_err(...) do { } while (0)
#define al_warn(...) do { } while (0)
#define al_info(...) do { } while (0)
#define al_dbg(...) do { } while (0)
#define al_assert(c) assert(c)
static inline void al_data_memory_barrier(void) { mock_mb(); }
static inline void al_smp_data_memory_barrier(void) { }
static inline void al_local_data_memory_barrier(void) { mock_mb(); }
#define al_udelay(u) mock_udelay(u)
#define al_msleep(m) mock_udelay((m) * 1000)
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
