// SPDX-License-Identifier: GPL-2.0-only
/* Minimal BCM98548XMC control-plane bring-up. No CPLD/flash/ASIC writes. */
#include <linux/init.h>
#include <linux/bug.h>
#include <asm/machdep.h>
#include <asm/mpic.h>
#include <asm/udbg.h>
#include <sysdev/fsl_pci.h>
#include "mpc85xx.h"

static void __init bcm98548xmc_init_irq(void)
{
	struct mpic *mpic;

	mpic = mpic_alloc(NULL, 0, MPIC_BIG_ENDIAN, 0, 256, "BCM98548XMC MPIC");
	BUG_ON(!mpic);
	mpic_init(mpic);
}

machine_arch_initcall(bcm98548xmc, mpc85xx_common_publish_devices);

static void __init bcm98548xmc_setup_arch(void)
{
	fsl_pci_assign_primary();
}

define_machine(bcm98548xmc) {
	.name = "Broadcom BCM98548XMC (netboot)",
	.compatible = "brcm,bcm98548xmc",
	.setup_arch = bcm98548xmc_setup_arch,
	.init_IRQ = bcm98548xmc_init_irq,
	.get_irq = mpic_get_irq,
	.progress = udbg_progress,
};
