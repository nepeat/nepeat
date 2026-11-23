// SPDX-License-Identifier: GPL-2.0
/*
 * No code: compile-time checks that the declarations reboot.rs writes by hand
 * match the kernel headers. linux/reboot.h is not part of the kernel's Rust
 * bindings (rust/bindings/bindings_helper.h) and an out-of-tree module cannot
 * add to them, so the C compiler checks them here instead of bindgen.
 */
#include <linux/build_bug.h>
#include <linux/notifier.h>
#include <linux/reboot.h>

static_assert(SYS_RESTART == 0x0001);
static_assert(__builtin_types_compatible_p(typeof(register_reboot_notifier),
					   int(struct notifier_block *)));
static_assert(__builtin_types_compatible_p(typeof(unregister_reboot_notifier),
					   int(struct notifier_block *)));
