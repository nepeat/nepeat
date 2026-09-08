// SPDX-License-Identifier: GPL-2.0
/* Read-only probe of the MT8183 BootROM-retained SRAM window. */
#include <linux/errno.h>
#include <linux/init.h>
#include <linux/io.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include <linux/types.h>
#include <asm/pgtable-hwdef.h>

#define YACHT_BROM_RETAINED_PA   0x00100000ULL
#define YACHT_BROM_RETAINED_SIZE 0x30

extern unsigned long kallsyms_lookup_name(const char *name);

static void __iomem *(*yacht_ioremap)(phys_addr_t, size_t, pgprot_t);
static void (*yacht_iounmap)(volatile void __iomem *);

static int __init yacht_sram_probe_init(void)
{
	void __iomem *sram;
	unsigned int offset;

	yacht_ioremap = (void *)kallsyms_lookup_name("__ioremap");
	yacht_iounmap = (void *)kallsyms_lookup_name("__iounmap");
	if (!yacht_ioremap || !yacht_iounmap)
		return -ENOENT;

	sram = yacht_ioremap(YACHT_BROM_RETAINED_PA,
			     YACHT_BROM_RETAINED_SIZE,
			     __pgprot(PROT_DEVICE_nGnRE));
	if (!sram)
		return -ENOMEM;

	pr_info("yacht_sram_probe: read-only BROM-retained SRAM\n");
	for (offset = 0; offset < YACHT_BROM_RETAINED_SIZE; offset += 16)
		pr_info("yacht_sram_probe: +%02x %08x %08x %08x %08x\n",
			offset,
			__raw_readl(sram + offset),
			__raw_readl(sram + offset + 4),
			__raw_readl(sram + offset + 8),
			__raw_readl(sram + offset + 12));
	yacht_iounmap(sram);
	return 0;
}

static void __exit yacht_sram_probe_exit(void)
{
	pr_info("yacht_sram_probe: unloaded\n");
}

module_init(yacht_sram_probe_init);
module_exit(yacht_sram_probe_exit);
MODULE_DESCRIPTION("Read-only MT8183 BootROM-retained SRAM probe");
MODULE_AUTHOR("ai_re");
MODULE_LICENSE("GPL");
