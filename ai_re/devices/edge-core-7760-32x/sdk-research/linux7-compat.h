/* Linux 7.x API compatibility for the SDK 6.5.35 compile probe.
 * Timer renames preserve semantics; hardware operation is not validated.
 */
#ifndef EDGECORE_SDK_LINUX7_COMPAT_H
#define EDGECORE_SDK_LINUX7_COMPAT_H
#include <linux/version.h>
#include <linux/timer.h>
#if LINUX_VERSION_CODE >= KERNEL_VERSION(7, 0, 0)
#define del_timer_sync timer_delete_sync
#define del_timer timer_delete
#define from_timer timer_container_of
#endif
#endif
