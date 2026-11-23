/* SPDX-License-Identifier: GPL-2.0 */
/*
 * al_eth_trace: the vendor al_eth driver with every HAL register access, PCI
 * config access and HAL delay recorded (al_trace_rec.c). Included at the end
 * of al_hal_plat_services.h by default.nix, it replaces the accessor macros.
 */
#ifndef AL_ETH_TRACE_H
#define AL_ETH_TRACE_H
#include <linux/types.h>

struct pci_dev;

enum al_trace_mark {
	AL_TRACE_PROBE, AL_TRACE_OPEN, AL_TRACE_CLOSE, AL_TRACE_REMOVE, AL_TRACE_RX_MODE,
	AL_TRACE_MTU, AL_TRACE_SET_MAC, AL_TRACE_STATS, AL_TRACE_BARS, AL_TRACE_NMARKS
};

u32 al_trace_r32(const volatile void __iomem *p);
void al_trace_w32(volatile void __iomem *p, u32 v);
u32 al_trace_r32_relaxed(const volatile void __iomem *p);
void al_trace_w32_relaxed(volatile void __iomem *p, u32 v);
u16 al_trace_r16(const volatile void __iomem *p);
void al_trace_w16(volatile void __iomem *p, u16 v);
u8 al_trace_r8(const volatile void __iomem *p);
void al_trace_w8(volatile void __iomem *p, u8 v);
void al_trace_udelay(unsigned long us);
void al_trace_cfg(int write, int where, u32 v);
void al_trace_mark(enum al_trace_mark m);
void al_trace_bars(struct pci_dev *pdev);
int al_trace_init(void);
void al_trace_exit(void);

#undef al_reg_read8
#undef al_reg_read16
#undef al_reg_read32
#undef al_reg_write8
#undef al_reg_write16
#undef al_reg_write32
#undef al_reg_read32_relaxed
#undef al_reg_write32_relaxed
#undef al_udelay
#define al_reg_read8(l)			al_trace_r8(l)
#define al_reg_read16(l)		al_trace_r16(l)
#define al_reg_read32(l)		al_trace_r32(l)
#define al_reg_write8(l, v)		al_trace_w8(l, v)
#define al_reg_write16(l, v)		al_trace_w16(l, v)
#define al_reg_write32(l, v)		al_trace_w32(l, v)
#define al_reg_read32_relaxed(l)	al_trace_r32_relaxed(l)
#define al_reg_write32_relaxed(l, v)	al_trace_w32_relaxed(l, v)
#define al_udelay(u)			al_trace_udelay(u)
#endif
