/*
 * Host test of include/spi_flash_window.h (from the U-Boot patch series):
 * the range check behind the SPI NOR write window and the spi-mem command
 * filter, with the DMC-12 window 0xf90000 + 0x20000 (env pair at 0xf90000 /
 * 0xfa0000). Fixed cases at every boundary, then 2M random requests against
 * a 128-bit reference model.
 */
#include <assert.h>
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

	/* command filter (spi-mem): WREN/WRDI only without address or data */
	CHECK(spi_flash_window_op_ok(0x06, 0, 0, SFW_NONE, 0, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0x04, 0, 0, SFW_NONE, 0, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0x06, 3, 0xf90000, SFW_NONE, 0, WS, WZ), 0);
	/* page program */
	CHECK(spi_flash_window_op_ok(0x02, 3, 0xf90000, SFW_OUT, 256, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0x02, 3, 0xfaff00, SFW_OUT, 256, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0x02, 3, 0xfaff01, SFW_OUT, 256, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0x02, 3, 0xf8ff00, SFW_OUT, 256, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0x02, 3, 0xfd0000, SFW_OUT, 1, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0x02, 3, 0xf90000, SFW_IN, 256, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0x02, 0, 0xf90000, SFW_OUT, 256, WS, WZ), 0);
	/* erases: the whole aligned block must be inside */
	CHECK(spi_flash_window_op_ok(0x20, 3, 0xf90000, SFW_NONE, 0, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0x20, 3, 0xfaf000, SFW_NONE, 0, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0x20, 3, 0xf8f000, SFW_NONE, 0, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0x20, 3, 0xfb0000, SFW_NONE, 0, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0xd8, 3, 0xf90000, SFW_NONE, 0, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0xd8, 3, 0xfaffff, SFW_NONE, 0, WS, WZ), 1);	/* block 0xfa0000 */
	CHECK(spi_flash_window_op_ok(0xd8, 3, 0xf8ffff, SFW_NONE, 0, WS, WZ), 0);	/* block 0xf80000 */
	CHECK(spi_flash_window_op_ok(0xd8, 3, 0xfd0000, SFW_NONE, 0, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0x52, 3, 0xfa8000, SFW_NONE, 0, WS, WZ), 1);
	CHECK(spi_flash_window_op_ok(0x52, 3, 0xf88000, SFW_NONE, 0, WS, WZ), 0);
	CHECK(spi_flash_window_op_ok(0x20, 3, 0xf90000, SFW_OUT, 1, WS, WZ), 0);
	/* never: SR/CR writes, chip erase, reset, lock, OTP, suspend, 4B mode, SR2 */
	static const unsigned char never[] = { 0x01, 0x31, 0x11, 0x2f, 0x68, 0x98, 0x7e, 0x36,
		0x39, 0x42, 0x44, 0xc7, 0x60, 0x66, 0x99, 0xb7, 0xe9, 0xb0, 0x30, 0x75, 0x7a,
		0xb1, 0xc1, 0xa5, 0x50, 0xc5 };
	for (unsigned i = 0; i < sizeof(never); i++)
		for (int d = SFW_NONE; d <= SFW_OUT; d++) {
			CHECK(spi_flash_window_op_ok(never[i], 0, 0, d, d ? 1 : 0, WS, WZ), 0);
			CHECK(spi_flash_window_op_ok(never[i], 3, 0xf90000, d, d ? 1 : 0, WS, WZ), 0);
		}

	/* random requests against the reference model */
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
		unsigned long long a = (n & 1 ? WS - 0x20000 + r64() % 0x60000 : r64() & 0xffffffff);
		unsigned int sz = spi_flash_window_erase_size(0x20);
		int ok = spi_flash_window_op_ok(0x20, 3, a, SFW_NONE, 0, WS, WZ);
		if (ok != ref_covers(a & ~(unsigned long long)(sz - 1), sz)) {
			printf("FAIL random erase %#llx\n", a);
			return 1;
		}
		ok = spi_flash_window_op_ok(0x02, 3, a, SFW_OUT, l % 512, WS, WZ);
		if (ok != ref_covers(a, l % 512)) {
			printf("FAIL random program %#llx+%#llx\n", a, l % 512);
			return 1;
		}
	}
	if (fails) {
		printf("FAIL: %d case(s)\n", fails);
		return 1;
	}
	printf("window: ok (boundaries, factory env and UBOOT ranges refused, chip/SR/lock/reset ops refused, 2M random requests match the model)\n");
	return 0;
}
