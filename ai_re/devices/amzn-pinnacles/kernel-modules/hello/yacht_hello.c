// SPDX-License-Identifier: GPL-2.0
#include <linux/init.h>
#include <linux/module.h>

static int __init yacht_hello_init(void)
{
	pr_info("yacht_hello: exact 4.4.146 module loaded\n");
	return 0;
}

static void __exit yacht_hello_exit(void)
{
	pr_info("yacht_hello: unloaded\n");
}

module_init(yacht_hello_init);
module_exit(yacht_hello_exit);

MODULE_DESCRIPTION("Non-destructive module ABI probe for Amazon yacht");
MODULE_AUTHOR("ai_re");
MODULE_LICENSE("GPL");

