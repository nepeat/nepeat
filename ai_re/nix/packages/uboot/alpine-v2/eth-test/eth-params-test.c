// SPDX-License-Identifier: GPL-2.0+
/*
 * Host unit test: the U-Boot side's al_eth handoff encoding
 * (include/alpine_v2_eth.h of the patched tree) against the Annapurna HAL
 * itself (al_eth_board_params_set/get, al_eth_mac_addr_store/read and the
 * register structs from nix/packages/al-eth/src), plus the ONIE TlvInfo
 * MAC parser against synthetic images.
 *
 * Built by tests.nix with the HAL headers next to host shims for
 * al_hal_plat_{types,services}.h; registers are plain memory.
 */
#include <stdlib.h>
#include <zlib.h>

#include "al_hal_eth.h"
#include "al_hal_eth_mac_regs.h"
#include "al_hal_eth_ec_regs.h"
#include "alpine_v2_eth.h"

#include "params_extract.c"
#include "mac_extract.c"

static int failures;

#define CHECK(c, ...) do { if (!(c)) { failures++; printf("FAIL %s:%d: ", __FILE__, __LINE__); \
	printf(__VA_ARGS__); printf("\n"); } } while (0)

static struct al_eth_mac_regs mac_regs;
static struct al_ec_regs ec_regs;

static void hal_encode(const struct alpine_v2_eth_params *p, uint32_t r[3])
{
	struct al_eth_board_params hp;

	memset(&hp, 0, sizeof(hp));
	hp.media_type = p->media_type;
	hp.phy_exist = p->phy_exist ? AL_TRUE : AL_FALSE;
	hp.phy_mdio_addr = p->phy_mdio_addr;
	hp.mdio_freq = p->mdio_freq;
	hp.phy_if = p->phy_if;
	hp.an_mode = p->an_mode_in_band ? AL_ETH_BOARD_AUTONEG_IN_BAND :
					  AL_ETH_BOARD_AUTONEG_OUT_OF_BAND;
	hp.ref_clk_freq = p->ref_clk_freq;
	hp.an_disable = p->an_disable ? AL_TRUE : AL_FALSE;
	hp.speed = p->speed;
	hp.half_duplex = p->half_duplex ? AL_TRUE : AL_FALSE;
	memset(&mac_regs, 0xa5, sizeof(mac_regs));
	al_eth_board_params_set(&mac_regs, &hp);
	r[0] = mac_regs.mac_1g.scratch;
	r[1] = mac_regs.mac_10g.scratch;
	r[2] = mac_regs.mac_1g.mac_0;
}

static void check_one(const struct alpine_v2_eth_params *p, const char *what)
{
	unsigned int ours[3] = { 0 };
	uint32_t hal[3];
	struct al_eth_board_params back;

	CHECK(!alpine_v2_eth_params_encode(p, ours), "%s: encode refused", what);
	hal_encode(p, hal);
	CHECK(ours[0] == hal[0] && ours[1] == hal[1] && ours[2] == hal[2],
	      "%s: ours %08x %08x %08x, HAL %08x %08x %08x", what,
	      ours[0], ours[1], ours[2], hal[0], hal[1], hal[2]);

	/* What Linux al_eth reads back from our registers. */
	memset(&mac_regs, 0, sizeof(mac_regs));
	mac_regs.mac_1g.scratch = ours[0];
	mac_regs.mac_10g.scratch = ours[1];
	mac_regs.mac_1g.mac_0 = ours[2];
	memset(&back, 0x5a, sizeof(back));
	CHECK(!al_eth_board_params_get(&mac_regs, &back), "%s: HAL get failed", what);
	CHECK(back.media_type == p->media_type && back.phy_exist == !!p->phy_exist &&
	      back.phy_mdio_addr == p->phy_mdio_addr && back.mdio_freq == p->mdio_freq &&
	      back.phy_if == p->phy_if && back.an_mode == !!p->an_mode_in_band &&
	      back.ref_clk_freq == p->ref_clk_freq && back.an_disable == !!p->an_disable &&
	      back.speed == p->speed && back.half_duplex == !!p->half_duplex &&
	      !back.sfp_plus_module_exist && !back.autoneg_enable && !back.kr_lt_enable &&
	      !back.serdes_grp && !back.serdes_lane && !back.retimer_exist &&
	      !back.dont_override_serdes && !back.force_1000_base_x,
	      "%s: HAL get disagrees", what);
}

static unsigned int tlv_crc(unsigned int c, const unsigned char *p, unsigned int n)
{
	return crc32(c, p, n);
}

/* type/len/value list -> TlvInfo image with a correct CRC TLV */
static unsigned int tlv_build(unsigned char *b, const unsigned char *tlvs, unsigned int n)
{
	unsigned int len = n + 6, c;

	memcpy(b, "TlvInfo", 8);
	b[8] = 1;
	b[9] = len >> 8;
	b[10] = len & 0xff;
	memcpy(b + 11, tlvs, n);
	b[11 + n] = 0xfe;
	b[12 + n] = 4;
	c = crc32(0, b, 13 + n);
	b[13 + n] = c >> 24;
	b[14 + n] = c >> 16;
	b[15 + n] = c >> 8;
	b[16 + n] = c;
	return 17 + n;
}

int main(void)
{
	struct alpine_v2_eth_params p;
	unsigned int r[3] = { 0 }, i, lo, hi;
	unsigned char img[256], mac[6], back[6];
	uint8_t hal_mac[6];
	uint32_t seed = 12345;

	/* Register offsets the U-Boot side hard-codes. */
	CHECK(offsetof(struct al_eth_mac_regs, mac_1g.scratch) == ALPINE_V2_ETH_PARAMS1, "params1");
	CHECK(offsetof(struct al_eth_mac_regs, mac_10g.scratch) == ALPINE_V2_ETH_PARAMS2, "params2");
	CHECK(offsetof(struct al_eth_mac_regs, mac_1g.mac_0) == ALPINE_V2_ETH_PARAMS3, "params3");
	CHECK(offsetof(struct al_ec_regs, fwd_mac[0].data_l) == ALPINE_V2_ETH_FWD_MAC0_L, "mac lo");
	CHECK(offsetof(struct al_ec_regs, fwd_mac[0].data_h) == ALPINE_V2_ETH_FWD_MAC0_H, "mac hi");
	CHECK(AL_ETH_BOARD_MEDIA_TYPE_RGMII == ALPINE_V2_ETH_MEDIA_RGMII &&
	      AL_ETH_BOARD_MEDIA_TYPE_SGMII == ALPINE_V2_ETH_MEDIA_SGMII &&
	      AL_ETH_BOARD_1G_SPEED_1000M == ALPINE_V2_ETH_SPEED_1000M &&
	      AL_ETH_BOARD_1G_SPEED_100M == ALPINE_V2_ETH_SPEED_100M &&
	      AL_ETH_BOARD_1G_SPEED_10M == ALPINE_V2_ETH_SPEED_10M &&
	      AL_ETH_BOARD_MDIO_FREQ_2_5_MHZ == ALPINE_V2_ETH_MDIO_2_5MHZ &&
	      AL_ETH_BOARD_PHY_IF_MDIO == ALPINE_V2_ETH_PHY_IF_MDIO &&
	      AL_ETH_REF_FREQ_500_MHZ == ALPINE_V2_ETH_REF_CLK_500MHZ, "enum values");

	/* The DMC-12 port: what the U-Boot DT (rgmii, fixed-link 1000 full) gives. */
	memset(&p, 0, sizeof(p));
	p.media_type = ALPINE_V2_ETH_MEDIA_RGMII;
	p.ref_clk_freq = ALPINE_V2_ETH_REF_CLK_500MHZ;
	p.an_disable = 1;
	p.speed = ALPINE_V2_ETH_SPEED_1000M;
	check_one(&p, "dmc12");
	alpine_v2_eth_params_encode(&p, r);
	printf("dmc12 board params: %08x %08x %08x\n", r[0], r[1], r[2]);
	CHECK(r[0] == 0x60000001 && r[1] == 0x00000004 && r[2] == 0, "dmc12 constants");

	/* Every field over its range, then random combinations. */
	for (i = 0; i < 20000; i++) {
		seed = seed * 1103515245 + 12345;
		p.media_type = 1 + (seed >> 8) % 15;
		p.phy_exist = (seed >> 12) & 1;
		p.phy_mdio_addr = (seed >> 13) & 0x1f;
		p.mdio_freq = (seed >> 18) & 3;
		p.phy_if = (seed >> 20) & 3;
		p.an_mode_in_band = (seed >> 22) & 1;
		p.ref_clk_freq = (seed >> 23) & 7;
		p.an_disable = (seed >> 26) & 1;
		p.speed = (seed >> 27) % 3;
		p.half_duplex = (seed >> 30) & 1;
		check_one(&p, "random");
	}
	memset(&p, 0, sizeof(p));
	CHECK(alpine_v2_eth_params_encode(&p, r), "all-zero params1 must be refused");
	p.media_type = 16;
	CHECK(alpine_v2_eth_params_encode(&p, r), "out-of-range media must be refused");

	/* MAC entry 0 */
	for (i = 0; i < 1000; i++) {
		unsigned int j;

		for (j = 0; j < 6; j++) {
			seed = seed * 1103515245 + 12345;
			mac[j] = seed >> 16;
		}
		alpine_v2_eth_mac_encode(mac, &lo, &hi);
		memset(&ec_regs, 0, sizeof(ec_regs));
		al_eth_mac_addr_store(&ec_regs, 0, mac);
		CHECK(ec_regs.fwd_mac[0].data_l == lo && ec_regs.fwd_mac[0].data_h == hi,
		      "mac encode %08x %08x vs HAL %08x %08x", lo, hi,
		      ec_regs.fwd_mac[0].data_l, ec_regs.fwd_mac[0].data_h);
		al_eth_mac_addr_read(&ec_regs, 0, hal_mac);
		alpine_v2_eth_mac_decode(lo, hi, back);
		CHECK(!memcmp(hal_mac, mac, 6) && !memcmp(back, mac, 6), "mac decode");
	}

	/* ONIE TlvInfo */
	{
		static const unsigned char good[] = {
			0x21, 3, 'D', 'M', 'C',
			0x24, 6, 0x02, 0x11, 0x22, 0x33, 0x44, 0x55,
			0x2a, 2, 0, 1,
		};
		static const unsigned char nomac[] = { 0x21, 3, 'D', 'M', 'C' };
		unsigned int n;

		memset(img, 0xff, sizeof(img));
		n = tlv_build(img, good, sizeof(good));
		memset(mac, 0, 6);
		CHECK(!alpine_v2_onie_tlv_mac(img, sizeof(img), mac, tlv_crc) &&
		      mac[0] == 0x02 && mac[5] == 0x55, "tlv good");
		img[13] ^= 1;	/* corrupt a value byte */
		CHECK(alpine_v2_onie_tlv_mac(img, sizeof(img), mac, tlv_crc) == -2, "tlv crc");
		n = tlv_build(img, nomac, sizeof(nomac));
		CHECK(alpine_v2_onie_tlv_mac(img, sizeof(img), mac, tlv_crc) == -3, "tlv no mac");
		img[0] = 'X';
		CHECK(alpine_v2_onie_tlv_mac(img, sizeof(img), mac, tlv_crc) == -1, "tlv header");
		n = tlv_build(img, good, sizeof(good));
		img[n - 6] = 0x99;	/* CRC TLV type -> unknown: no CRC TLV */
		CHECK(alpine_v2_onie_tlv_mac(img, sizeof(img), mac, tlv_crc) == -4, "tlv no crc");
		n = tlv_build(img, good, sizeof(good));
		img[10] = 0xff;	/* total length beyond the 256-byte part */
		CHECK(alpine_v2_onie_tlv_mac(img, sizeof(img), mac, tlv_crc) == -1, "tlv length");
		(void)n;
	}

	if (failures) {
		printf("%d failure(s)\n", failures);
		return 1;
	}
	printf("eth params: ok (offsets, enums, DMC-12 constants, 20000 random param sets "
	       "vs HAL set/get, 1000 MACs vs HAL store/read, ONIE TLV cases)\n");
	return 0;
}
