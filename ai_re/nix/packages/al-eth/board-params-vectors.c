// SPDX-License-Identifier: GPL-2.0
/*
 * Host program: test vectors for rust/board_params.rs out of the HAL's own
 * al_eth_board_params_get()/_set() (extracted from src/al_hal_eth_main.c by
 * rust.nix; registers are plain memory, as in ../uboot/alpine-v2/eth-test).
 *   D r1 r2 r3 rc f0..f31    get() of the registers (fields only when rc is 0)
 *   E f0..f31 r1 r2 r3       set() of the fields
 * Fields in the order of struct al_eth_board_params.
 */
#include <stdlib.h>

#include "al_hal_eth.h"
#include "al_hal_eth_mac_regs.h"

#include "params_extract.c"

static struct al_eth_mac_regs mac_regs;

#define FIELDS(X) \
	X(media_type) X(phy_exist) X(phy_mdio_addr) X(sfp_plus_module_exist) \
	X(autoneg_enable) X(kr_lt_enable) X(kr_fec_enable) X(mdio_freq) \
	X(i2c_adapter_id) X(phy_if) X(an_mode) X(serdes_grp) X(serdes_lane) \
	X(ref_clk_freq) X(dont_override_serdes) X(force_1000_base_x) \
	X(an_disable) X(speed) X(half_duplex) X(fc_disable) X(retimer_exist) \
	X(retimer_bus_id) X(retimer_i2c_addr) X(retimer_channel) X(dac) \
	X(dac_len) X(retimer_type) X(retimer_tx_channel) X(gpio_sfp_present) \
	X(auto_fec_enable) X(gpio_spd_1g_10g) X(gpio_spd_25g)

static void print_fields(const struct al_eth_board_params *p)
{
#define X(f) printf(" %u", (unsigned int)p->f);
	FIELDS(X)
#undef X
}

static uint32_t rnd(void)
{
	static uint64_t s = 0x9e3779b97f4a7c15ull;

	s ^= s << 13;
	s ^= s >> 7;
	s ^= s << 17;
	return (uint32_t)(s >> 16);
}

static void decode(uint32_t r1, uint32_t r2, uint32_t r3)
{
	struct al_eth_board_params p;
	int rc;

	memset(&mac_regs, 0, sizeof(mac_regs));
	mac_regs.mac_1g.scratch = r1;
	mac_regs.mac_10g.scratch = r2;
	mac_regs.mac_1g.mac_0 = r3;
	memset(&p, 0x5a, sizeof(p));
	rc = al_eth_board_params_get(&mac_regs, &p);
	printf("D %u %u %u %d", r1, r2, r3, rc);
	if (!rc)
		print_fields(&p);
	printf("\n");
}

/* A field of the Rust struct: a bool (0/1) or a u8 (any value, set() masks it). */
static const char is_bool[] = {
	0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1,
	1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0,
};

static void encode(int small)
{
	struct al_eth_board_params p;
	uint32_t v[32];
	int i = 0;

	for (i = 0; i < 32; i++)
		v[i] = is_bool[i] ? rnd() & 1 : rnd() & (small ? 0x0f : 0xff);
	i = 0;
#define X(f) p.f = v[i++];
	FIELDS(X)
#undef X
	/* set() asserts that register 1 is not 0 */
	if (!(p.media_type & 0xf))
		p.media_type = 1;
	memset(&mac_regs, 0xa5, sizeof(mac_regs));
	al_eth_board_params_set(&mac_regs, &p);
	printf("E");
	print_fields(&p);
	printf(" %u %u %u\n", mac_regs.mac_1g.scratch, mac_regs.mac_10g.scratch,
	       mac_regs.mac_1g.mac_0);
}

int main(void)
{
	int i, r;

	decode(0, 0, 0);
	decode(0, 0xffffffff, 0xffffffff);
	decode(0xffffffff, 0xffffffff, 0xffffffff);
	decode(0x60000001, 0x00000004, 0); /* DMC-12 */
	for (r = 0; r < 3; r++)
		for (i = 0; i < 32; i++) {
			uint32_t regs[3] = { 1, 0, 0 };

			regs[r] ^= 1u << i;
			decode(regs[0], regs[1], regs[2]);
		}
	for (i = 0; i < 200000; i++)
		decode(rnd(), rnd(), rnd());
	for (i = 0; i < 200000; i++)
		encode(i & 1);
	return 0;
}
