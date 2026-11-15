// SPDX-License-Identifier: GPL-2.0
/*
 * Edgecore DMC-12-48R-AS0: full board reset through the board CPLD on reboot.
 *
 * The CPLD sits on I2C (factory bus 4, behind the pca9548 at 0x70, channel 3)
 * at 0x68. Writing 0x29 <- 1 and then 0x28 <- 1 resets the whole board (USB
 * hubs, PHYs, uC), not only the SoC: the sequence of the factory kernel's
 * al_dmc12_restart(), disassembled from 4.19.32-al-linux-v10.2.0 (ai_re
 * devices/dmc-12-48r-as0/vendor-tooling.md). Verified on u01, 2026-11-15:
 * afterwards the CPLD WDT flag (0x2a) reads 0x03.
 *
 * This runs as a reboot notifier: inside reboot(2), after userspace is gone and
 * filesystems are synced, before device_shutdown(), in sleepable context, so a
 * normal SMBus transfer is fine. No i2c client is bound at 0x68, so userspace
 * (fan control, interrupt monitor, metrics) keeps raw i2c-dev access to the CPLD.
 * If the CPLD hasn't reset the board after 2 s, the notifier returns and the
 * kernel continues to the sp805 restart handler (SoC-only reset).
 */
#include <linux/delay.h>
#include <linux/i2c.h>
#include <linux/module.h>
#include <linux/reboot.h>

static int bus = 4;
module_param(bus, int, 0444);
MODULE_PARM_DESC(bus, "Linux I2C bus of the CPLD (DT alias i2c4)");

#define CPLD_ADDR 0x68

static int cpld_write(struct i2c_adapter *adap, u8 reg, u8 val)
{
	union i2c_smbus_data data = { .byte = val };

	return i2c_smbus_xfer(adap, CPLD_ADDR, 0, I2C_SMBUS_WRITE, reg,
			      I2C_SMBUS_BYTE_DATA, &data);
}

static int dmc12_reboot_notify(struct notifier_block *nb, unsigned long action, void *cmd)
{
	struct i2c_adapter *adap;
	int ret;

	if (action != SYS_RESTART)
		return NOTIFY_DONE;
	adap = i2c_get_adapter(bus);
	if (!adap) {
		pr_err("dmc12-cpld-reset: no i2c-%d, falling back\n", bus);
		return NOTIFY_DONE;
	}
	pr_emerg("dmc12-cpld-reset: board reset via CPLD\n");
	ret = cpld_write(adap, 0x29, 0x01);
	if (!ret)
		ret = cpld_write(adap, 0x28, 0x01);
	i2c_put_adapter(adap);
	if (ret) {
		pr_err("dmc12-cpld-reset: CPLD write failed (%d), falling back\n", ret);
		return NOTIFY_DONE;
	}
	mdelay(2000);
	pr_err("dmc12-cpld-reset: CPLD did not reset the board, falling back\n");
	return NOTIFY_DONE;
}

static struct notifier_block dmc12_reboot_nb = {
	.notifier_call = dmc12_reboot_notify,
	/* Run after other notifiers: they may still want the hardware. */
	.priority = INT_MIN,
};

static int __init dmc12_cpld_reset_init(void)
{
	return register_reboot_notifier(&dmc12_reboot_nb);
}

static void __exit dmc12_cpld_reset_exit(void)
{
	unregister_reboot_notifier(&dmc12_reboot_nb);
}

module_init(dmc12_cpld_reset_init);
module_exit(dmc12_cpld_reset_exit);
MODULE_DESCRIPTION("DMC-12 board reset through the CPLD on reboot");
MODULE_LICENSE("GPL");
