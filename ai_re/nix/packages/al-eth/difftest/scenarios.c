// SPDX-License-Identifier: GPL-2.0
/*
 * Scenario driver of the difftest: runs the vendor HAL (through glue.c) over
 * the mock register bank and prints the trace (mock.h has the format) to
 * stdout. ../rust.nix feeds that trace to the Rust replay test.
 */
#include <stdlib.h>
#include <string.h>
#include "mock.h"
#include "glue.h"
#include "al_hal_eth_mac_regs.h"
#include "al_hal_eth_ec_regs.h"
#include "al_hal_udma_regs.h"
#include "al_hal_iofic_regs.h"

#define OFFU(m) ((uint32_t)offsetof(struct unit_regs, m))
#define QTX(q, f) (OFFU(m2s.m2s_q[q]) + (uint32_t)offsetof(union udma_q_regs, rings.f))
#define QRX(q, f) (OFFU(s2m.s2m_q[q]) + (uint32_t)offsetof(union udma_q_regs, rings.f))

static void call_end(void) { mock_ring_dump(); }

static void board(uint32_t r1, uint32_t r2, uint32_t r3)
{
	mock_set(BAR_M, offsetof(struct al_eth_mac_regs, mac_1g.scratch), r1);
	mock_set(BAR_M, offsetof(struct al_eth_mac_regs, mac_10g.scratch), r2);
	mock_set(BAR_M, offsetof(struct al_eth_mac_regs, mac_1g.mac_0), r3);
	mock_set(BAR_E, offsetof(struct al_ec_regs, fwd_mac[0].data_l), 0xb69d38c0);
	mock_set(BAR_E, offsetof(struct al_ec_regs, fwd_mac[0].data_h), 0x000034ef);
}

#define DMC12 0x60000001, 0x00000004, 0x00000000
#define CS8210_SGMII 0x60004013, 0x00000000, 0x00000000
#define CS8210_10G 0x60000002, 0x00000000, 0x00000000

/* registers a filled bank must keep sane so the polls end: UDMA states idle, queues stopped */
static void pin_idle(void)
{
	int q;

	mock_pin(BAR_U, OFFU(m2s.m2s.state), 0);
	mock_pin(BAR_U, OFFU(s2m.s2m.state), 0);
	for (q = 0; q < 4; q++) {
		mock_pin(BAR_U, QTX(q, status), 0);
		mock_pin(BAR_U, QTX(q, dcp), 0);
		mock_pin(BAR_U, QTX(q, crhp), 0);
		mock_pin(BAR_U, QRX(q, status), 0);
		mock_pin(BAR_U, QRX(q, dcp), 0);
		mock_pin(BAR_U, QRX(q, crhp), 0);
	}
}

static int c_probe(void) { int rc; fprintf(mock_out, "I CALL probe 2\n"); rc = g_probe(2); call_end(); return rc; }
static void c_up(void)
{
	int i;
	fprintf(mock_out, "I CALL up");
	for (i = 0; i < 10; i++) {
		g.hash_key[i] = rnd();
		fprintf(mock_out, " %08x", g.hash_key[i]);
	}
	fprintf(mock_out, "\n");
	g_up();
	call_end();
}
static void c_down(void) { fprintf(mock_out, "I CALL down\n"); g_down(); call_end(); }
static void c_stats(void) { fprintf(mock_out, "I CALL stats\n"); g_stats(); call_end(); }
static void c_link(void) { fprintf(mock_out, "I CALL link\n"); g_link_status(); call_end(); }
static void c_mgmt(void) { fprintf(mock_out, "I CALL mgmt\n"); g_mgmt_irq(); call_end(); }

static void lifecycle(const char *name, uint32_t seed, uint32_t r1, uint32_t r2, uint32_t r3)
{
	mock_scenario(name);
	if (seed) {
		mock_fill(seed);
		pin_idle();
	}
	board(r1, r2, r3);
	c_probe();
	c_up();
	c_stats();
	c_link();
	c_down();
	c_up();
	c_down();
}

/*
 * The DMC-12's registers as al_eth found them at probe (first value of every
 * register read, from the al_eth_trace recording of the C driver on the box,
 * 2026-11-23, devices/dmc-12-48r-as0/al-eth-hw-trace-diff.txt; the zero ones
 * left out). UDMA revision 0x1c360002 makes al_udma_set_defaults take its
 * rev >= 2 branch, which the zero-bank scenarios do not.
 */
static const struct { int bar; uint32_t off, val; } dmc12_regs[] = {
		{ BAR_M, 0x00004, 0x60000001 },
		{ BAR_M, 0x00404, 0x00000004 },
		{ BAR_E, 0x00868, 0xb69d38c0 },
		{ BAR_E, 0x0086c, 0x000034ef },
		{ BAR_U, 0x1e084, 0x1c360002 },
		{ BAR_U, 0x00310, 0x00200100 },
		{ BAR_M, 0x00008, 0x01800018 },
		{ BAR_U, 0x01020, 0x03000000 },
		{ BAR_U, 0x01040, 0x40000000 },
		{ BAR_U, 0x0104c, 0x40000000 },
		{ BAR_U, 0x02020, 0x03000000 },
		{ BAR_U, 0x02040, 0x40000000 },
		{ BAR_U, 0x0204c, 0x40000000 },
		{ BAR_U, 0x03020, 0x03000000 },
		{ BAR_U, 0x03040, 0x40000000 },
		{ BAR_U, 0x0304c, 0x40000000 },
		{ BAR_U, 0x04020, 0x03000000 },
		{ BAR_U, 0x04040, 0x40000000 },
		{ BAR_U, 0x0404c, 0x40000000 },
		{ BAR_U, 0x11020, 0x03000033 },
		{ BAR_U, 0x11040, 0x40000000 },
		{ BAR_U, 0x1104c, 0x40000000 },
		{ BAR_U, 0x12020, 0x03000033 },
		{ BAR_U, 0x12040, 0x40000000 },
		{ BAR_U, 0x1204c, 0x40000000 },
		{ BAR_U, 0x13020, 0x03000033 },
		{ BAR_U, 0x13040, 0x40000000 },
		{ BAR_U, 0x1304c, 0x40000000 },
		{ BAR_U, 0x14020, 0x03000033 },
		{ BAR_U, 0x14040, 0x40000000 },
		{ BAR_U, 0x1404c, 0x40000000 },
		{ BAR_M, 0x00964, 0x00063910 },
		{ BAR_M, 0x0095c, 0x00000800 },
		{ BAR_U, 0x00254, 0x01010000 },
		{ BAR_U, 0x00304, 0x00010010 },
		{ BAR_U, 0x00308, 0x00000841 },
		{ BAR_U, 0x10308, 0x00000841 },
		{ BAR_U, 0x10120, 0x00080010 },
		{ BAR_E, 0x00098, 0x00050000 },
		{ BAR_E, 0x00b00, 0x00013c01 },
};

static void board_state(void)
{
	size_t i;

	mock_scenario("dmc12_board");
	for (i = 0; i < sizeof(dmc12_regs) / sizeof(dmc12_regs[0]); i++)
		mock_set(dmc12_regs[i].bar, dmc12_regs[i].off, dmc12_regs[i].val);
	c_probe();
	c_up();
	c_stats();
	c_link();
	c_down();
	c_up();
	c_down();
}

static void rx_mode(void)
{
	int i;

	mock_scenario("rx_mode");
	board(DMC12);
	c_probe();
	c_up();
	for (i = 0; i < 300; i++) {
		int p = rnd() % 4 == 0, a = rnd() % 3 == 0, mc = rnd() % 3, uc = rnd() % 7;
		fprintf(mock_out, "I CALL rx_mode %d %d %d %d\n", p, a, mc, uc);
		g_set_rx_mode(p, a, mc, uc);
		call_end();
	}
	c_down();
}

static void set_mac(void)
{
	int i, j;
	uint8_t a[6];

	mock_scenario("set_mac");
	board(DMC12);
	c_probe();
	for (i = 0; i < 100; i++) {
		for (j = 0; j < 6; j++)
			a[j] = rnd();
		a[0] &= 0xfe;
		fprintf(mock_out, "I CALL set_mac %02x:%02x:%02x:%02x:%02x:%02x\n", a[0], a[1], a[2], a[3], a[4], a[5]);
		g_set_mac(a);
		call_end();
		if (i == 50) {
			c_up();
			c_down();
		}
	}
}

static void mtu(void)
{
	int i, m;

	mock_scenario("mtu");
	board(DMC12);
	c_probe();
	for (i = 0; i < 120; i++) {
		m = (i < 6) ? (int[]){ 67, 68, 1500, 9216, 9217, -1 }[i] : (int)(rnd() % 9400);
		fprintf(mock_out, "I CALL mtu %d\n", m);
		g_change_mtu(m);
		call_end();
		if (i % 40 == 39) {   /* the Rx buffer size follows the MTU */
			c_up();
			c_down();
		}
	}
}

static void stats(const char *name, uint32_t r1, uint32_t r2, uint32_t r3)
{
	int i;
	uint32_t off;

	mock_scenario(name);
	board(r1, r2, r3);
	c_probe();
	c_stats();
	c_up();
	for (i = 0; i < 6; i++) {
		for (off = 0x60; off < 0xe4; off += 4)
			mock_set(BAR_M, off, rnd());
		for (off = 0x480; off < 0x594; off += 4)
			mock_set(BAR_M, off, rnd() >> (rnd() % 32));
		mock_set(BAR_M, offsetof(struct al_eth_mac_regs, stat.eee_in), rnd());
		mock_set(BAR_M, offsetof(struct al_eth_mac_regs, stat.eee_out), rnd());
		c_stats();
	}
	c_down();
}

static void link(const char *name, uint32_t r1, uint32_t r2, uint32_t r3)
{
	int i;

	mock_scenario(name);
	board(r1, r2, r3);
	c_probe();
	c_up();
	for (i = 0; i < 40; i++) {
		mock_set(BAR_M, offsetof(struct al_eth_mac_regs, kr.pcs_data), rnd());
		mock_set(BAR_M, offsetof(struct al_eth_mac_regs, mac_10g.status), rnd());
		mock_set(BAR_M, offsetof(struct al_eth_mac_regs, sgmii.reg_data), rnd());
		mock_set(BAR_M, offsetof(struct al_eth_mac_regs, sgmii.link_stat), rnd());
		mock_set(BAR_M, offsetof(struct al_eth_mac_regs, gen.rgmii_stat), rnd());
		c_link();
	}
	c_down();
}

static void mac_config(void)
{
	static const int modes[] = { AL_ETH_MAC_MODE_RGMII, AL_ETH_MAC_MODE_SGMII, AL_ETH_MAC_MODE_10GbE_Serial };
	static const int speeds[] = { 10, 100, 1000, 2500 };
	int m, f, a, s, d;

	mock_scenario("mac_config");
	mock_fill(0x5eed0001);
	pin_idle();
	board(DMC12);
	c_probe();
	for (m = 0; m < 3; m++)
		for (f = 0; f < 2; f++)
			for (a = 0; a < 2; a++)
				for (s = 0; s < 4; s++)
					for (d = 0; d < 2; d++) {
						fprintf(mock_out, "I CALL mac_config %d %d %d %d %d\n", modes[m], f, a, speeds[s], d);
						g_mac_config(modes[m], f, a, speeds[s], d);
						call_end();
					}
}

static void flr(void)
{
	mock_scenario("flr");
	mock_fill(0x5eed0002);
	pin_idle();
	board(DMC12);
	c_probe();
	fprintf(mock_out, "I CALL flr\n");
	function_reset();
	call_end();
}

static void timeouts(void)
{
	/* queue resets that never finish: errors the driver ignores */
	mock_scenario("timeouts_queue");
	mock_pin(BAR_U, QTX(1, status), 1u << 28);   /* prefetch never stops */
	mock_pin(BAR_U, QRX(2, dcp), 5);             /* dcp != crhp */
	board(DMC12);
	if (c_probe() == 0) {
		c_up();
		c_down();
	}
	/* a UDMA that never goes idle: probe fails */
	mock_scenario("timeouts_state");
	mock_pin(BAR_U, OFFU(s2m.s2m.state), 1 << 4);   /* stream interface stays normal */
	board(DMC12);
	if (c_probe() == 0) {
		c_up();
		c_down();
	}
	/* board parameters missing */
	mock_scenario("no_params");
	c_probe();
}

static void mgmt(void)
{
	int i;

	mock_scenario("mgmt");
	board(DMC12);
	c_probe();
	c_up();
	for (i = 0; i < 10; i++) {
		/* int_cause of primary group D, secondary groups A and B */
		mock_set(BAR_U, OFFU(gen.interrupt_regs.main_iofic) + 3 * sizeof(struct al_iofic_grp_ctrl), rnd());
		mock_set(BAR_U, OFFU(gen.interrupt_regs.secondary_iofic_ctrl), rnd());
		mock_set(BAR_U, OFFU(gen.interrupt_regs.secondary_iofic_ctrl) + sizeof(struct al_iofic_grp_ctrl), rnd());
		c_mgmt();
	}
	c_down();
}

/* Tx: random skbs on random queues, the fake hardware completing descriptors in random steps */
static void tx(uint32_t seed, int npkts, int complete_every)
{
	char name[32];
	uint32_t submitted[4] = { 0 }, completed[4] = { 0 }, n = 0;
	int i, q;

	snprintf(name, sizeof(name), "tx_%08x", seed);
	mock_scenario(name);
	rnd_seed(seed);
	board(DMC12);
	c_probe();
	c_up();
	for (i = 0; i < npkts; i++) {
		struct gskb sk;
		int f;
		uint32_t pending;

		q = rnd() % 4;
		if (g.tx[q].stopped) {   /* the stack does not send on a stopped queue */
			completed[q] = submitted[q];
			mock_set(BAR_U, QTX(q, crhp), completed[q] & 0xffff);
			fprintf(mock_out, "I CALL tx_poll %d\n", q);
			g_tx_poll(q);
			call_end();
			continue;
		}
		memset(&sk, 0, sizeof(sk));
		sk.nr_frags = (rnd() % 8 == 0) ? (int)(rnd() % 18) : (int)(rnd() % 3);
		sk.headlen = 1 + rnd() % 1514;
		sk.len = sk.headlen;
		for (f = 0; f < sk.nr_frags; f++) {
			sk.frag_len[f] = 1 + rnd() % 4096;
			sk.len += sk.frag_len[f];
		}
		for (f = 0; f <= sk.nr_frags; f++)
			sk.addr[f] = 0x3000000000ull + ((uint64_t)q << 32) + (uint64_t)(n % 4096) * 0x10000 + (uint64_t)f * 0x1000 + (rnd() & 0x3f);
		n++;
		sk.partial = rnd() % 2;
		sk.gso_size = (rnd() % 4 == 0) ? 1 + rnd() % 1600 : 0;
		sk.ip_version = (int[]){ 4, 4, 6, 6, 0 }[rnd() % 5];
		sk.l4proto = (int[]){ 6, 6, 17, 1 }[rnd() % 4];
		sk.vlan = rnd() % 5 == 0;
		sk.nh_off = 14 + (sk.vlan ? 4 : 0);
		sk.nh_len = (sk.ip_version == 6 ? 40 : 20) + 4 * (rnd() % 3);
		sk.transport_off = sk.nh_off + sk.nh_len;
		sk.doff = rnd() % 16;
		fprintf(mock_out, "I CALL xmit %d %u %u %d", q, sk.len, sk.headlen, sk.nr_frags);
		for (f = 0; f < sk.nr_frags; f++)
			fprintf(mock_out, " %u", sk.frag_len[f]);
		fprintf(mock_out, " %d %u %d %d %u %u %u %u %d", sk.partial, sk.gso_size, sk.ip_version,
			sk.l4proto, sk.nh_off, sk.nh_len, sk.transport_off, sk.doff, sk.vlan);
		for (f = 0; f <= sk.nr_frags; f++)
			fprintf(mock_out, " %llx", (unsigned long long)sk.addr[f]);
		fprintf(mock_out, "\n");
		{
			uint32_t before = g.txq[q]->next_desc_idx;
			g_xmit(q, &sk);
			submitted[q] += (g.txq[q]->next_desc_idx - before) & 1023;
		}
		call_end();
		/* the hardware completes some descriptors; the driver polls now and then or when stopped */
		pending = submitted[q] - completed[q];
		if (pending && (rnd() % complete_every == 0 || g.tx[q].stopped)) {
			completed[q] += (rnd() % 4 == 0) ? pending : 1 + rnd() % pending;
			mock_set(BAR_U, QTX(q, crhp), completed[q] & 0xffff);
		}
		if (rnd() % (complete_every + 1) == 0 || g.tx[q].stopped) {
			fprintf(mock_out, "I CALL tx_poll %d\n", q);
			g_tx_poll(q);
			call_end();
		}
	}
	for (q = 0; q < 4; q++) {   /* drain: every buffer comes back before down */
		mock_set(BAR_U, QTX(q, crhp), submitted[q] & 0xffff);
		fprintf(mock_out, "I CALL tx_poll %d\n", q);
		g_tx_poll(q);
		call_end();
	}
	c_down();
}

/* Rx: the fake hardware writes completion descriptors for posted buffers */
static void rx(uint32_t seed, int rounds)
{
	char name[32];
	int i, q;

	snprintf(name, sizeof(name), "rx_%08x", seed);
	mock_scenario(name);
	rnd_seed(seed);
	board(DMC12);
	c_probe();
	c_up();
	for (i = 0; i < rounds; i++) {
		int k, npk;
		struct grx *r;

		q = rnd() % 4;
		r = &g.rx[q];
		npk = rnd() % 70;
		for (k = 0; k < npk; k++) {
			unsigned int posted = (g.rxq[q]->next_desc_idx - r->hw_idx) & 1023;
			unsigned int nd = (rnd() % 6 == 0) ? 2 + rnd() % 3 : 1, d;
			/* l3 index 4:0 (IPv4 8, IPv6 11), l4 index 12:8 (TCP 12, UDP 13), csum errors 13/14 */
			static const uint32_t l3s[] = { 8, 8, 11, 11, 0, 5 }, l4s[] = { 12, 12, 13, 0, 3 };
			uint32_t flags = l3s[rnd() % 6] | (l4s[rnd() % 5] << 8) | ((rnd() % 4 == 0) ? (rnd() & (3u << 13)) : 0);
			if (rnd() % 16 == 0)
				flags |= 1u << 16;
			if (rnd() % 64 == 0)
				flags |= 1u << 31;
			if (nd > posted)
				break;
			for (d = 0; d < nd; d++) {
				uint32_t w[4];
				int partial = (k == npk - 1) && (d == nd - 1) && (nd > 1) && (rnd() % 3 == 0);
				/* ring id 25:24, FIRST 26, LAST 27, DDP 29, BUF1_USED 30 (al_hal_udma.h) */
				w[0] = flags | (r->hw_ring_id << 24) |
				       (d == 0 ? (1u << 26) : 0) | (d == nd - 1 ? (1u << 27) : 0) | (rnd() % 8 ? (1u << 30) : 0) |
				       (rnd() % 32 == 0 ? (1u << 29) : 0);
				w[1] = (rnd() & 0xffff0000u) | ((rnd() % 3 == 0) ? (1 + rnd() % 128) : (1 + rnd() % 1536));
				w[2] = rnd();
				w[3] = rnd();
				if (partial)
					break;   /* the rest of this packet arrives in a later round */
				mock_cdesc(r->cring, r->hw_idx, w);
				r->hw_idx = (r->hw_idx + 1) & 1023;
				if (r->hw_idx == 0)
					r->hw_ring_id = (r->hw_ring_id + 1) & 3;
			}
		}
		{
			int budget = 1 + (int)(rnd() % 64);
			fprintf(mock_out, "I CALL rx_poll %d %d\n", q, budget);
			g_rx_poll(q, budget);
		}
		call_end();
	}
	c_down();
}

int main(int argc, char **argv)
{
	(void)argc;
	(void)argv;
	mock_init();
	fprintf(mock_out, "# al_eth difftest trace (vendor HAL via difftest/glue.c)\n");
	lifecycle("dmc12_lifecycle", 0, DMC12);
	lifecycle("dmc12_filled_a", 0x00c0ffee, DMC12);
	lifecycle("dmc12_filled_b", 0x12345678, DMC12);
	lifecycle("cs8210_sgmii", 0, CS8210_SGMII);
	lifecycle("cs8210_10g", 0, CS8210_10G);
	lifecycle("cs8210_10g_filled", 0xfeedface, CS8210_10G);
	board_state();
	rx_mode();
	set_mac();
	mtu();
	stats("stats_rgmii", DMC12);
	stats("stats_sgmii", CS8210_SGMII);
	stats("stats_10g", CS8210_10G);
	link("link_rgmii", DMC12);
	link("link_sgmii", CS8210_SGMII);
	link("link_10g", CS8210_10G);
	mac_config();
	flr();
	timeouts();
	mgmt();
	tx(0x7a7a0001, 3000, 3);
	tx(0x7a7a0002, 3000, 3);
	tx(0x7a7a0003, 4000, 400);   /* sparse completions: queues fill up and stop */
	rx(0x5a5a0001, 400);
	rx(0x5a5a0002, 400);
	fprintf(mock_out, "# end\n");
	return 0;
}
