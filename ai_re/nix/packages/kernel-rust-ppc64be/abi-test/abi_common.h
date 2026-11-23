/* SPDX-License-Identifier: GPL-2.0 */
/* Value and hash helpers shared by the C side; abi_main.rs has the Rust twins. */
#ifndef ABI_COMMON_H
#define ABI_COMMON_H
#include <linux/types.h>
#include <linux/string.h>
#include <linux/stddef.h>

#define ABI_H0 0xcbf29ce484222325ULL

static inline u64 abi_mix(u64 h, u64 v)
{
	return (h ^ v) * 0x100000001b3ULL;
}

/* splitmix64 */
static inline u64 abi_sm(u64 x)
{
	u64 z = x + 0x9e3779b97f4a7c15ULL;

	z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9ULL;
	z = (z ^ (z >> 27)) * 0x94d049bb133111ebULL;
	return z ^ (z >> 31);
}
#endif
