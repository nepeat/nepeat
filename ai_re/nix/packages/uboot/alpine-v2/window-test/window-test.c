/*
 * Host test of include/spi_flash_window.h (from the U-Boot patch series):
 * the range check behind the SPI NOR write window and the spi-mem command
 * filter, with the DMC-12 window 0xf90000 + 0x20000 (env pair at 0xf90000 /
 * 0xfa0000).
 *  - fixed cases at every boundary (factory env and UBOOT ranges refused);
 *  - every opcode x address width 0..8 x dummy 0..2 x direction x a few
 *    addresses: the allowed set equals the whitelist, nothing else passes;
 *  - random requests (3- and 4-byte program, 4K/32K/64K erase in both
 *    widths, mismatched widths) against a 128-bit reference model.
 */
#include <stdio.h>
#include <stdlib.h>
#include "spi_flash_window.h"

#define WS 0xf90000ULL
#define WZ 0x20000ULL
static int fails;

#define CHECK(expr, want) do { int got_ = !!(expr); if (got_ != (want)) { \
	printf("FAIL %s:%d: %s = %d, want %d\n", __FILE__, __LINE__, #expr, got_, (want)); \
	fails++; } } while (0)

static int ref_covers(unsigned long long s, unsigned long long l)
{
	unsigned __int128 e = (unsigned __int128)s + l;
	return l && s >= WS && e <= (unsigned __int128)(WS + WZ);
}

/* the whitelist, written out independently of the header */
static int ref_op(unsigned op, unsigned an, unsigned long long a, unsigned dm, int dir,
		  unsigned long long n)
{
	unsigned w3 = 0, w4 = 0, esz = 0;

	if (op == 0x06 || op == 0x04)
		return an == 0 && dm == 0 && dir == SFW_NONE;
	switch (op) {
	case 0x02: w3 = 1; break;
	case 0x12: w4 = 1; break;
	case 0x20: w3 = 1; esz = 0x1000; break;
	case 0x21: w4 = 1; esz = 0x1000; break;
	case 0x52: w3 = 1; esz = 0x8000; break;
	case 0x5c: w4 = 1; esz = 0x8000; break;
	case 0xd8: w3 = 1; esz = 0x10000; break;
	case 0xdc: w4 = 1; esz = 0x10000; break;
	default: return 0;
	}
	if (dm || (w3 && an != 3) || (w4 && an != 4))
		return 0;
	if (!esz)
		return dir == SFW_OUT && ref_covers(a, n);
	return dir == SFW_NONE && n == 0 && ref_covers(a / esz * esz, esz);
}

static unsigned long long r64(void)
{
	unsigned long long v = 0;
	for (int i = 0; i < 4; i++)
		v = v << 16 ^ (rand() & 0xffff);
	return v;
}

int main(void)
{
	/* whole-request check (spi-nor MTD hooks) */
	CHECK(spi_flash_window_covers(0xf90000, 0x10000, WS, WZ), 1);	/* env */
	CHECK(spi_flash_window_covers(0xfa0000, 0x10000, WS, WZ), 1);	/* env redund */
	CHECK(spi_flash_window_covers(0xf90000, 0x20000, WS, WZ), 1);	/* whole window */
	CHECK(spi_flash_window_covers(0xfaffff, 1, WS, WZ), 1);
	CHECK(spi_flash_window_covers(0xf8ffff, 1, WS, WZ), 0);	/* 1 below */
	CHECK(spi_flash_window_covers(0xf8ffff, 2, WS, WZ), 0);	/* straddles start */
	CHECK(spi_flash_window_covers(0xfb0000, 1, WS, WZ), 0);	/* 1 past end */
	CHECK(spi_flash_window_covers(0xfaffff, 2, WS, WZ), 0);	/* straddles end */
	CHECK(spi_flash_window_covers(0xf90000, 0x20001, WS, WZ), 0);
	CHECK(spi_flash_window_covers(0xf90000, 0, WS, WZ), 0);	/* empty */
	CHECK(spi_flash_window_covers(0xfd0000, 0x10000, WS, WZ), 0);	/* factory env */
	CHECK(spi_flash_window_covers(0xfe0000, 0x10000, WS, WZ), 0);	/* factory env redund */
	CHECK(spi_flash_window_covers(0, 0x1000000, WS, WZ), 0);	/* whole chip */
	CHECK(spi_flash_window_covers(0x1c0000, 0xe0000, WS, WZ), 0);	/* factory UBOOT 0 */
	CHECK(spi_flash_window_covers(0xf90000, ~0ULL, WS, WZ), 0);	/* overflow */
	CHECK(spi_flash_window_covers(0xfa0000, 0xffffffffffff0000ULL, WS, WZ), 0);
	CHECK(spi_flash_window_covers(~0ULL, 2, WS, WZ), 0);
	/* the review scenario: 4-byte address with a 3-byte program opcode */
	CHECK(spi_flash_window_op_ok(0x02, 4, 0x00f90000, 0, SFW_OUT, 1, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0x02, 3, 0xf90000, 0, SFW_OUT, 256, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0x12, 4, 0xf90000, 0, SFW_OUT, 256, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0x12, 3, 0xf90000, 0, SFW_OUT, 256, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0xd8, 3, 0xfaffff, 0, SFW_NONE, 0, WS, WZ), 1);	/* block 0xfa0000 */
	CHECK(spi_flash_window_op_ok(0xd8, 3, 0xf8ffff, 0, SFW_NONE, 0, WS, WZ), 0);	/* block 0xf80000 */
	CHECK(spi_flash_window_op_ok(0x20, 3, 0xf90000, 1, SFW_NONE, 0, WS, WZ), 0);	/* dummy */

	/* exhaustive: allowed set == whitelist */
	static const unsigned long long addrs[] = { 0, 0xf8f000, 0xf8ffff, 0xf90000, 0xf9ff00,
		0xfa0000, 0xfaf000, 0xfaff00, 0xfaffff, 0xfb0000, 0xfd0000, 0x1f90000,
		0xffffffffULL };
	static const unsigned long long lens[] = { 0, 1, 256, 0x10000, 0x20001 };
	long allowed = 0, checked = 0;
	for (unsigned op = 0; op < 256; op++)
		for (unsigned an = 0; an <= 8; an++)
			for (unsigned dm = 0; dm <= 2; dm++)
				for (int dir = SFW_NONE; dir <= SFW_OUT; dir++)
					for (unsigned ai = 0; ai < sizeof(addrs) / sizeof(addrs[0]); ai++)
						for (unsigned li = 0; li < sizeof(lens) / sizeof(lens[0]); li++) {
							int got = spi_flash_window_op_ok(op, an, addrs[ai], dm, dir,
											 lens[li], WS, WZ);
							int want = ref_op(op, an, addrs[ai], dm, dir, lens[li]);
							checked++;
							allowed += got;
							if (got != want) {
								printf("FAIL op %02x an %u a %#llx dm %u dir %d n %#llx: %d, want %d\n",
								       op, an, addrs[ai], dm, dir, lens[li], got, want);
								return 1;
							}
						}

	/* random requests against the reference model */
	static const unsigned char rops[] = { 0x02, 0x12, 0x20, 0x21, 0x52, 0x5c, 0xd8, 0xdc };
	srand(12);
	for (long n = 0; n < 2000000; n++) {
		unsigned long long s, l;
		switch (n % 4) {
		case 0: s = r64(); l = r64(); break;
		case 1: s = WS - 0x40000 + r64() % 0x80000; l = r64() % 0x40000; break;
		case 2: s = WS + r64() % WZ; l = r64() % (WZ + 2); break;
		default: s = r64(); l = r64() % 0x1000; break;
		}
		if (spi_flash_window_covers(s, l, WS, WZ) != ref_covers(s, l)) {
			printf("FAIL random: start %#llx len %#llx\n", s, l);
			return 1;
		}
		unsigned char op = rops[n % 8];
		unsigned an = 3 + (rand() & 1);			/* matching or not */
		unsigned long long a = (n & 1 ? WS - 0x30000 + r64() % 0x80000 : r64() & 0x1ffffff);
		int pp = op == 0x02 || op == 0x12;
		unsigned long long dl = pp ? l % 512 : 0;
		int dir = pp ? SFW_OUT : SFW_NONE;
		if (spi_flash_window_op_ok(op, an, a, 0, dir, dl, WS, WZ) != ref_op(op, an, a, 0, dir, dl)) {
			printf("FAIL random op %02x an %u %#llx+%#llx\n", op, an, a, dl);
			return 1;
		}
	}
	if (fails) {
		printf("FAIL: %d case(s)\n", fails);
		return 1;
	}
	printf("window: ok (boundaries, factory env and UBOOT ranges refused; %ld opcode/width/dummy/dir/range "
	       "combinations, %ld allowed, all matching the whitelist; 2M random requests incl. 4-byte, "
	       "32K/64K and width mismatches match the model)\n", checked, allowed);
	return 0;
}
