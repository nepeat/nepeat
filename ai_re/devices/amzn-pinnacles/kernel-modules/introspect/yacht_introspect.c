// SPDX-License-Identifier: GPL-2.0
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>

extern unsigned long kallsyms_lookup_name(const char *name);

static const char * const yacht_symbols[] = {
	"machine_shutdown",
	"device_shutdown",
	"migrate_to_reboot_cpu",
	"kernel_restart_prepare",
	"smp_send_stop",
	"__flush_dcache_area",
	"idmap_pg_dir",
	"secondary_holding_pen",
	"secondary_holding_pen_release",
	"mtk_wdt_stop",
	"mtk_wdt_ping",
	"watchdog_stop",
	"idme_get_dev_flags_value",
	"idme_get_item",
	"force_ro_store",
	"sys_kexec_load",
	"sys_kexec_file_load",
};

static int __init yacht_introspect_init(void)
{
	size_t i;

	pr_info("yacht_introspect: resolving warm-boot prerequisites\n");
	for (i = 0; i < ARRAY_SIZE(yacht_symbols); i++)
		pr_info("yacht_introspect: %s=%px\n", yacht_symbols[i],
			(void *)kallsyms_lookup_name(yacht_symbols[i]));

	return 0;
}

static void __exit yacht_introspect_exit(void)
{
	pr_info("yacht_introspect: unloaded\n");
}

module_init(yacht_introspect_init);
module_exit(yacht_introspect_exit);

MODULE_DESCRIPTION("Read-only private-symbol probe for Amazon yacht");
MODULE_AUTHOR("ai_re");
MODULE_LICENSE("GPL");
