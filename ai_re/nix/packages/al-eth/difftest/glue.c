// SPDX-License-Identifier: GPL-2.0
/*
 * The hardware-facing part of al_eth_main.c (the Linux glue of the vendor
 * driver, ../src), restated without the kernel: what the DMC-12 / CS8210
 * paths do to the HAL, in the same order and with the same arguments. The
 * vendor HAL itself (../src/al_hal_*.c) is linked unmodified. Every function
 * names the al_eth_main.c function it restates; kernel-only work (skb, NAPI,
 * IRQ requests, DMA mapping) is reduced to the decisions it makes, which are
 * printed as RET lines. Keep this file in step with ../src/al_eth_main.c.
 */
#include <stdlib.h>
#include <string.h>
#include "al_hal_eth.h"
#include "al_hal_eth_mac_regs.h"
#include "al_hal_eth_ec_regs.h"
#include "al_hal_udma_iofic.h"
#include "al_hal_udma_config.h"
#include "mock.h"
#include "glue.h"

#define BIT(n) (1u << (n))
#define AL_UDMA_INITIAL_RING_ID 1   /* al_hal_udma_main.c */
#define NQ 4
#define RING 1024
#define MAX_SKB_FRAGS 17               /* CONFIG_MAX_SKB_FRAGS default (7.3) */
#define SMALL_COPY_LEN 128             /* AL_ETH_DEFAULT_SMALL_PACKET_LEN, NET_IP_ALIGN = 0 on arm64 */
#define MAX_RX_BUFF 1536
#define MIN_RX_BUFF 600
#define TX_WAKEUP_THRESH (MAX_SKB_FRAGS + 2)
#define MAC_TABLE_UC_BASE 0
#define MAC_TABLE_UC_MAX 4
#define MAC_TABLE_ALL_MC (MAC_TABLE_UC_BASE + MAC_TABLE_UC_MAX)
#define MAC_TABLE_DROP (AL_ETH_FWD_MAC_NUM - 1)
#define MAC_TABLE_BCAST (MAC_TABLE_DROP - 1)
#define FLOW_CTRL_RX_PAUSE BIT(1)
#define FLOW_CTRL_TX_PAUSE BIT(2)

struct glue g;

static uint32_t ref_clk_hz(int f)
{
	switch (f) {
	case AL_ETH_REF_FREQ_375_MHZ: return 375000000;
	case AL_ETH_REF_FREQ_500_MHZ: return 500000000;
	case AL_ETH_REF_FREQ_187_5_MHZ: return 187500000;
	case AL_ETH_REF_FREQ_250_MHZ: return 250000000;
	case AL_ETH_REF_FREQ_428_MHZ: return 428000000;
	default: return 500000000;
	}
}

static unsigned int res_nsecs(void)
{
	unsigned int sb = ref_clk_hz(g.ref_clk_freq) / 1000;
	return (2 * 256 * 1000000u) / sb;   /* AL_ETH_INTR_MODERATION_RESOLUTION = 1 */
}

/* synthetic DMA addresses: what dma_map_single() would hand back */
/* every 61st allocation attempt of a queue fails */
int rx_alloc_fails(int q, uint32_t attempt) { (void)q; return attempt % 61 == 60; }
uint64_t rx_buf_addr(int q, uint32_t n) { return 0x2000000000ull + ((uint64_t)q << 28) + (uint64_t)(n % 0x10000) * 0x1000; }

/* al_eth_board_params_init, ALPINE_INTEGRATED */
static int board_params_init(void)
{
	struct al_eth_board_params p;

	if (al_eth_board_params_get(g.mac, &p))
		return -1;
	g.phy_exist = p.phy_exist == AL_TRUE;
	g.an_en = p.autoneg_enable;
	g.serdes_lane = p.serdes_lane;
	g.ref_clk_freq = p.ref_clk_freq;
	g.duplex = !p.half_duplex;
	g.autoneg = g.phy_exist ? (p.an_mode == AL_ETH_BOARD_AUTONEG_IN_BAND) : !p.an_disable;
	g.force_1000_base_x = p.force_1000_base_x;
	g.phy_if = p.phy_if;
	switch (p.speed) {
	default: g.speed = 1000; break;
	case AL_ETH_BOARD_1G_SPEED_1000M: g.speed = 1000; break;
	case AL_ETH_BOARD_1G_SPEED_100M: g.speed = 100; break;
	case AL_ETH_BOARD_1G_SPEED_10M: g.speed = 10; break;
	}
	g.mdio_freq = p.mdio_freq == AL_ETH_BOARD_MDIO_FREQ_1_MHZ ? 1000 : 2500;
	switch (p.media_type) {
	case AL_ETH_BOARD_MEDIA_TYPE_RGMII:
		g.mac_mode = p.sfp_plus_module_exist == AL_TRUE ? AL_ETH_MAC_MODE_SGMII : AL_ETH_MAC_MODE_RGMII;
		break;
	case AL_ETH_BOARD_MEDIA_TYPE_SGMII:
		g.mac_mode = AL_ETH_MAC_MODE_SGMII;
		break;
	case AL_ETH_BOARD_MEDIA_TYPE_10GBASE_SR:
		g.mac_mode = AL_ETH_MAC_MODE_10GbE_Serial;
		break;
	default:
		return -1;   /* other media: not on our boards */
	}
	al_eth_mac_addr_read(g.ec, 0, g.mac_addr);
	return 0;
}

/* al_eth_hal_adapter_init */
static void hal_adapter_init(void)
{
	struct al_hal_eth_adapter *h = &g.hal;
	struct al_udma_params up;
	int i;

	h->rev_id = g.rev_id;
	h->udma_id = 0;
	h->udma_regs_base = g.udma;
	h->ec_regs_base = g.ec;
	h->mac_regs_base = g.mac;
	h->unit_regs = g.udma;
	h->enable_rx_parser = 1;
	h->serdes_lane = g.serdes_lane;
	h->ec_ints_base = (uint8_t *)g.ec + 0x1c00;
	h->mac_ints_base = (void *)((uint8_t *)g.mac + 0x800);
	up.udma_regs_base = g.udma;
	up.type = UDMA_TX;
	up.num_of_queues = AL_ETH_UDMA_TX_QUEUES;
	up.name = "eth tx";
	al_udma_init(&h->tx_udma, &up);
	up.type = UDMA_RX;
	up.num_of_queues = AL_ETH_UDMA_RX_QUEUES;
	up.name = "eth rx";
	al_udma_init(&h->rx_udma, &up);
	for (i = 0; i < NQ; i++) {
		al_udma_q_handle_get(&h->tx_udma, i, &g.txq[i]);
		g.txq[i]->q_regs = (union udma_q_regs *)&h->tx_udma.udma_regs->m2s.m2s_q[i];
		g.txq[i]->qid = i;
		g.txq[i]->udma = &h->tx_udma;
	}
	for (i = 0; i < NQ; i++) {
		al_udma_q_handle_get(&h->rx_udma, i, &g.rxq[i]);
		g.rxq[i]->q_regs = (union udma_q_regs *)&h->rx_udma.udma_regs->s2m.s2m_q[i];
		g.rxq[i]->qid = i;
		g.rxq[i]->udma = &h->rx_udma;
	}
}

/* al_eth_flow_ctrl_config */
static void flow_ctrl_config(void)
{
	struct al_eth_flow_control_params fc;
	int i;

	memset(&fc, 0, sizeof(fc));
	fc.type = AL_ETH_FLOW_CONTROL_TYPE_LINK_PAUSE;
	fc.obay_enable = (g.flow_ctrl_active & FLOW_CTRL_RX_PAUSE) != 0;
	fc.gen_enable = (g.flow_ctrl_active & FLOW_CTRL_TX_PAUSE) != 0;
	fc.rx_fifo_th_high = 0x160;
	fc.rx_fifo_th_low = 0x90;
	fc.quanta = 0xffff;
	fc.quanta_th = 0x8000;
	for (i = 0; i < AL_ETH_FWD_PRIO_TABLE_NUM; i++)
		fc.prio_q_map[0][i] = 1 << (i >> 1);
	al_eth_flow_control_config(&g.hal, &fc);
}

static void mac_table_set(uint8_t idx, const uint8_t *addr, const uint8_t *mask, uint8_t udma_mask, int filter)
{
	struct al_eth_fwd_mac_table_entry e;

	memset(&e, 0, sizeof(e));
	memcpy(e.addr, addr, 6);
	memcpy(e.mask, mask, 6);
	e.rx_valid = AL_TRUE;
	e.tx_valid = AL_FALSE;
	e.udma_mask = udma_mask;
	e.filter = filter ? AL_TRUE : AL_FALSE;
	al_eth_fwd_mac_table_set(&g.hal, idx, &e);
}

static const uint8_t ones[6] = { 0xff, 0xff, 0xff, 0xff, 0xff, 0xff };
static const uint8_t zeros[6];

/* al_eth_mac_table_unicast_add: note it programs adapter->mac_addr, not addr */
static void mac_table_unicast_add(uint8_t idx, uint8_t udma_mask) { mac_table_set(idx, g.mac_addr, ones, udma_mask, 0); }
static void mac_table_broadcast_add(uint8_t idx, uint8_t udma_mask) { mac_table_set(idx, ones, ones, udma_mask, 0); }
static void mac_table_all_multicast_add(uint8_t idx, uint8_t udma_mask)
{
	static const uint8_t mc[6] = { 1 };
	mac_table_set(idx, mc, mc, udma_mask, 0);
}
static void mac_table_promiscuous_set(int promisc, uint8_t udma_mask)
{
	mac_table_set(MAC_TABLE_DROP, zeros, zeros, promisc ? udma_mask : 0, !promisc);
}
static void mac_table_entry_clear(uint8_t idx)
{
	struct al_eth_fwd_mac_table_entry e;
	memset(&e, 0, sizeof(e));
	al_eth_fwd_mac_table_set(&g.hal, idx, &e);
}

/* al_eth_hw_stop */
static void hw_stop(void)
{
	int i;

	al_eth_mac_stop(&g.hal);
	al_udelay(100);
	for (i = 0; i < NQ; i++)
		if (g.txq[i] && g.txq[i]->q_regs)
			al_udma_q_reset(g.txq[i]);
	for (i = 0; i < NQ; i++)
		if (g.rxq[i] && g.rxq[i]->q_regs)
			al_udma_q_reset(g.rxq[i]);
	al_eth_adapter_stop(&g.hal);
	g.reset_requested = 1;
	g.flow_ctrl_active = 0;   /* al_eth_flow_ctrl_disable */
	flow_ctrl_config();
}

/* al_eth_function_reset, udma 0 */
void function_reset(void)
{
	struct al_eth_board_params p;

	al_eth_board_params_get(g.mac, &p);
	al_eth_mac_addr_read(g.ec, 0, g.mac_addr);
	al_eth_flr_rmn(mock_pci_read, mock_pci_write, NULL, g.mac);
	al_eth_board_params_set(g.mac, &p);
	al_eth_mac_addr_store(g.ec, 0, g.mac_addr);
}

/* al_eth_hw_init_adapter, integrated (not NIC) */
static int hw_init_adapter(void)
{
	struct al_eth_adapter_params *pr = &g.params;

	pr->rev_id = g.rev_id;
	pr->udma_id = 0;
	pr->enable_rx_parser = 1;
	pr->udma_regs_base = g.udma;
	pr->ec_regs_base = g.ec;
	pr->mac_regs_base = g.mac;
	pr->name = "al_eth_0";
	pr->serdes_lane = g.serdes_lane;
	return al_eth_adapter_init(&g.hal, pr);
}

/* al_eth_init_rings: the unmask register and value of each queue's NAPI */
static void init_rings(void)
{
	int i;

	for (i = 0; i < NQ; i++) {
		g.tx_unmask_off = (uint32_t)((uint8_t *)al_udma_iofic_unmask_offset_get(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_C) - (uint8_t *)g.udma);
		g.rx_unmask_off = (uint32_t)((uint8_t *)al_udma_iofic_unmask_offset_get(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_B) - (uint8_t *)g.udma);
		g.rx[i].mod_idx = 0;
		g.rx[i].smoothed = 1000 * 0 / res_nsecs();   /* AL_ETH_INTR_LOWEST_VALUE */
		g.rx[i].packets = 0;
		g.rx[i].bytes = 0;
	}
}

/* al_eth_probe (hardware part) */
int g_probe(int rev_id)
{
	int i;
	unsigned int ns;

	memset(&g, 0, sizeof(g));
	g.udma = mock_bar_mem[BAR_U];
	g.ec = mock_bar_mem[BAR_E];
	g.mac = mock_bar_mem[BAR_M];
	g.rev_id = rev_id;
	g.mtu = 1500;
	if (board_params_init()) {
		fprintf(mock_out, "RET probe failed\n");
		return -1;
	}
	hal_adapter_init();
	g.hal.mac_mode = g.mac_mode;
	hw_stop();
	function_reset();
	if (hw_init_adapter()) {
		fprintf(mock_out, "RET probe failed\n");
		return -1;
	}
	init_rings();
	for (i = 0; i < 256; i++)
		g.rss_ind_tbl[i] = i % NQ;     /* ethtool_rxfh_indir_default */
	g.adaptive = 1;
	/* al_eth_init_intr_default_moderation_table_intervals + restore_default */
	ns = res_nsecs();
	g.mod[0].interval = 1000 * 0 / ns;   g.mod[0].pkts = 3;   g.mod[0].bytes = 2 * 1524;
	g.mod[1].interval = 1000 * 32 / ns;  g.mod[1].pkts = 12;  g.mod[1].bytes = 16 * 1024;
	g.mod[2].interval = 1000 * 80 / ns;  g.mod[2].pkts = 48;  g.mod[2].bytes = 64 * 1024;
	g.mod[3].interval = 1000 * 128 / ns; g.mod[3].pkts = 96;  g.mod[3].bytes = 128 * 1024;
	g.mod[4].interval = 1000 * 192 / ns; g.mod[4].pkts = 128; g.mod[4].bytes = 192 * 1024;
	fprintf(mock_out, "RET probe mode %d mac %02x:%02x:%02x:%02x:%02x:%02x speed %u duplex %d autoneg %d\n",
		g.mac_mode, g.mac_addr[0], g.mac_addr[1], g.mac_addr[2], g.mac_addr[3], g.mac_addr[4],
		g.mac_addr[5], g.speed, g.duplex, g.autoneg);
	return 0;
}

/* al_eth_hw_init */
static int hw_init(void)
{
	int rc = hw_init_adapter();

	if (rc)
		return rc;
	rc = al_eth_mac_config(&g.hal, g.mac_mode);
	if (rc < 0)
		return rc;
	if (g.mac_mode == AL_ETH_MAC_MODE_SGMII || (g.mac_mode == AL_ETH_MAC_MODE_RGMII && !g.phy_exist)) {
		int speed = g.speed, duplex = g.duplex;
		if (speed <= 0) { speed = 1000; duplex = 1; }
		rc = al_eth_mac_link_config(&g.hal, g.force_1000_base_x, g.autoneg, speed, duplex);
		if (rc)
			return rc;
	}
	rc = al_eth_mdio_config(&g.hal, g.phy_if == AL_ETH_BOARD_PHY_IF_XMDIO ?
				AL_ETH_MDIO_TYPE_CLAUSE_45 : AL_ETH_MDIO_TYPE_CLAUSE_22,
				AL_TRUE, g.ref_clk_freq, g.mdio_freq);
	if (rc)
		return rc;
	g.flow_ctrl_supported = FLOW_CTRL_TX_PAUSE | FLOW_CTRL_RX_PAUSE;   /* al_eth_flow_ctrl_init */
	return 0;
}

/* al_eth_set_coalesce */
static void set_coalesce(unsigned int tx_usecs, unsigned int rx_usecs)
{
	unsigned int ns = res_nsecs(), interval;
	int q;

	if (g.tx_usecs != tx_usecs) {
		interval = 1000 * tx_usecs / ns;
		g.tx_usecs = interval * ns / 1000;
		for (q = 0; q < NQ; q++)
			al_iofic_msix_moder_interval_config(&((struct unit_regs *)g.udma)->gen.interrupt_regs.main_iofic,
							    AL_INT_GROUP_C, q, interval);
	}
	if (g.rx_usecs != rx_usecs) {
		interval = 1000 * rx_usecs / ns;
		g.rx_usecs = interval * ns / 1000;
		for (q = 0; q < NQ; q++)
			al_iofic_msix_moder_interval_config(&((struct unit_regs *)g.udma)->gen.interrupt_regs.main_iofic,
							    AL_INT_GROUP_B, q, interval);
	}
}

/* al_eth_change_mtu */
static int change_mtu(int mtu)
{
	if (mtu < 68 || mtu > 9216)
		return -EINVAL;
	al_eth_rx_pkt_limit_config(&g.hal, AL_ETH_MIN_FRAME_LEN, mtu + 14 + 4 + 4);
	g.mtu = mtu;
	return 0;
}

int g_change_mtu(int mtu)
{
	int rc = change_mtu(mtu);

	fprintf(mock_out, rc ? "RET change_mtu einval\n" : "RET change_mtu ok\n");
	return rc;
}

static unsigned int rx_data_size(void)
{
	unsigned int s = g.mtu + 14 + 4 + 4;
	if (s > MAX_RX_BUFF) s = MAX_RX_BUFF;
	if (s < MIN_RX_BUFF) s = MIN_RX_BUFF;
	return s;
}

/* al_eth_refill_rx_bufs (+ al_eth_alloc_rx_frag: a slot keeps its buffer until a big packet takes it) */
static unsigned int refill_rx(int q, unsigned int num)
{
	struct grx *r = &g.rx[q];
	unsigned int i;
	uint16_t ntu = r->ntu;

	for (i = 0; i < num; i++) {
		struct al_buf b;
		struct gslot *s = &r->slot[ntu];
		if (!s->used) {
			if (rx_alloc_fails(q, r->attempts++)) {   /* netdev_alloc_frag failed */
				break;
			}
			s->used = 1;
			s->addr = rx_buf_addr(q, r->allocs++);
			s->len = rx_data_size();
		}
		b.addr = s->addr;
		b.len = s->len;
		if (al_eth_rx_buffer_add(g.rxq[q], &b, AL_ETH_RX_FLAGS_INT, NULL))
			break;
		ntu = (ntu + 1) & (RING - 1);
	}
	if (i)
		al_eth_rx_buffer_action(g.rxq[q], i);
	r->ntu = ntu;
	return i;
}

static void alloc_rings(void)
{
	int q;
	char name[8];

	mock_rings_clear();
	for (q = 0; q < NQ; q++) {
		free(g.tx[q].qp.desc_base);
		free(g.rx[q].qp.desc_base);
		free(g.rx[q].qp.cdesc_base);
	}
	for (q = 0; q < NQ; q++) {
		struct al_udma_q_params *p = &g.tx[q].qp;
		memset(p, 0, sizeof(*p));
		p->size = RING;
		p->desc_base = aligned_alloc(64, RING * 16);
		memset(p->desc_base, 0, RING * 16);
		p->desc_phy_base = 0x1000000000ull + (uint64_t)q * 0x100000;
		p->cdesc_base = NULL;
		p->cdesc_size = 8;
		snprintf(name, sizeof(name), "tx%d", q);
		g.tx[q].ring = mock_ring_add(name, p->desc_base, RING);
		g.tx[q].ntu = g.tx[q].ntc = 0;
		memset(g.tx[q].slot, 0, sizeof(g.tx[q].slot));
		g.tx[q].stopped = 0;
	}
	for (q = 0; q < NQ; q++) {
		struct al_udma_q_params *p = &g.rx[q].qp;
		memset(p, 0, sizeof(*p));
		p->size = RING;
		p->desc_base = aligned_alloc(64, RING * 16);
		memset(p->desc_base, 0, RING * 16);
		p->desc_phy_base = 0x1000800000ull + (uint64_t)q * 0x100000;
		p->cdesc_size = 16;
		p->cdesc_base = aligned_alloc(64, RING * 16);
		memset(p->cdesc_base, 0, RING * 16);
		p->cdesc_phy_base = 0x1001000000ull + (uint64_t)q * 0x100000;
		snprintf(name, sizeof(name), "rx%d", q);
		g.rx[q].ring = mock_ring_add(name, p->desc_base, RING);
		snprintf(name, sizeof(name), "rc%d", q);
		g.rx[q].cring = mock_ring_add(name, p->cdesc_base, RING);
		g.rx[q].ntu = g.rx[q].ntc = 0;
		memset(g.rx[q].slot, 0, sizeof(g.rx[q].slot));
		g.rx[q].hw_idx = 0;
		g.rx[q].hw_ring_id = AL_UDMA_INITIAL_RING_ID;
	}
}

/* al_eth_up + al_eth_up_complete (MSI-X per queue: 9 vectors granted) */
int g_up(void)
{
	int q, i, rc;
	struct al_eth_fwd_ctrl_table_entry entry;
	unsigned int tx_usecs, rx_usecs;

	if (g.reset_requested) {
		function_reset();
		g.reset_requested = 0;
	}
	rc = hw_init();
	if (rc) {
		fprintf(mock_out, "RET up hw_init %d\n", rc);
		return rc;
	}
	alloc_rings();
	/* al_eth_configure_int_mode */
	al_udma_iofic_config(g.udma, AL_IOFIC_MODE_MSIX_PER_Q, 0x480, 0x480, 0x1E0, 0x1E0);
	al_iofic_moder_res_config(&((struct unit_regs *)g.udma)->gen.interrupt_regs.main_iofic, AL_INT_GROUP_B, 1);
	al_iofic_moder_res_config(&((struct unit_regs *)g.udma)->gen.interrupt_regs.main_iofic, AL_INT_GROUP_C, 1);
	if (g.adaptive) {
		set_coalesce(196, 0);
		memcpy(g.modtab, g.mod, sizeof(g.mod));   /* al_eth_configure_intr_moderation_table */
	}
	/* al_eth_config_rx_fwd */
	for (i = 0; i < AL_ETH_FWD_PBITS_TABLE_NUM; i++)
		al_eth_fwd_pbits_table_set(&g.hal, i, i);
	for (i = 0; i < AL_ETH_FWD_PRIO_TABLE_NUM; i++)
		al_eth_fwd_priority_table_set(&g.hal, i, i >> 1);
	memset(&entry, 0, sizeof(entry));
	entry.prio_sel = AL_ETH_CTRL_TABLE_PRIO_SEL_VAL_0;
	entry.queue_sel_1 = AL_ETH_CTRL_TABLE_QUEUE_SEL_1_THASH_TABLE;
	entry.queue_sel_2 = AL_ETH_CTRL_TABLE_QUEUE_SEL_2_NO_PRIO;
	entry.udma_sel = AL_ETH_CTRL_TABLE_UDMA_SEL_MAC_TABLE;
	entry.filter = AL_FALSE;
	al_eth_ctrl_table_def_set(&g.hal, AL_FALSE, &entry);
	mac_table_unicast_add(MAC_TABLE_UC_BASE, 1);
	mac_table_broadcast_add(MAC_TABLE_BCAST, 1);
	mac_table_promiscuous_set(0, 1);
	for (i = 0; i < AL_ETH_RX_HASH_KEY_NUM; i++)
		al_eth_hash_key_set(&g.hal, i, __builtin_bswap32(g.hash_key[i]));   /* htonl */
	for (i = 0; i < AL_ETH_RX_THASH_TABLE_SIZE; i++)
		al_eth_thash_table_set(&g.hal, i, 0, g.rss_ind_tbl[i]);
	/* al_eth_fsm_table_init */
	for (i = 0; i < AL_ETH_RX_FSM_TABLE_SIZE; i++) {
		uint32_t val;
		switch (AL_ETH_FSM_ENTRY_OUTER(i)) {
		case AL_ETH_FSM_ENTRY_IPV4_TCP:
		case AL_ETH_FSM_ENTRY_IPV4_UDP:
		case AL_ETH_FSM_ENTRY_IPV6_TCP:
		case AL_ETH_FSM_ENTRY_IPV6_UDP:
			val = AL_ETH_FSM_DATA_OUTER_4_TUPLE | AL_ETH_FSM_DATA_HASH_SEL;
			break;
		case AL_ETH_FSM_ENTRY_IPV6_NO_UDP_TCP:
		case AL_ETH_FSM_ENTRY_IPV4_NO_UDP_TCP:
			val = AL_ETH_FSM_DATA_OUTER_2_TUPLE | AL_ETH_FSM_DATA_HASH_SEL;
			break;
		default:
			val = (0 << AL_ETH_FSM_DATA_DEFAULT_Q_SHIFT | ((1 << 0) << AL_ETH_FSM_DATA_DEFAULT_UDMA_SHIFT));
		}
		al_eth_fsm_table_set(&g.hal, i, val);
	}
	change_mtu(g.mtu);
	/* al_eth_udma_queues_enable_all */
	for (q = 0; q < NQ; q++)
		al_eth_queue_config(&g.hal, UDMA_TX, q, &g.tx[q].qp);
	for (q = 0; q < NQ; q++)
		al_eth_queue_config(&g.hal, UDMA_RX, q, &g.rx[q].qp);
	/* al_eth_refill_all_rx_bufs */
	for (q = 0; q < NQ; q++)
		refill_rx(q, RING - 1);
	/* al_eth_interrupts_unmask */
	al_udma_iofic_unmask(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_A, AL_INT_GROUP_A_GROUP_D_SUM);
	al_udma_iofic_unmask(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_B, (1 << NQ) - 1);
	al_udma_iofic_unmask(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_C, (1 << NQ) - 1);
	al_udma_iofic_unmask(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_D, 3 << 8);
	/* al_eth_flow_ctrl_enable */
	g.flow_ctrl_active = g.flow_ctrl_supported;
	flow_ctrl_config();
	/* al_eth_restore_ethtool_params */
	tx_usecs = g.tx_usecs;
	rx_usecs = g.rx_usecs;
	g.tx_usecs = 0;
	g.rx_usecs = 0;
	set_coalesce(tx_usecs, rx_usecs);
	for (i = 0; i < AL_ETH_RX_THASH_TABLE_SIZE; i++)
		al_eth_thash_table_set(&g.hal, i, 0, g.rss_ind_tbl[i]);
	al_eth_mac_start(&g.hal);
	g.up = 1;
	fprintf(mock_out, "RET up ok\n");
	return 0;
}

/* al_eth_down */
void g_down(void)
{
	int q;

	g.up = 0;
	/* al_eth_disable_int_sync -> al_eth_interrupts_mask */
	al_udma_iofic_mask(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_A, 0x7);
	al_udma_iofic_mask(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_B, 0xF);
	al_udma_iofic_mask(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_C, 0xF);
	al_udma_iofic_mask(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_D, 0xFFFFFFFF);
	hw_stop();
	/* al_eth_free_all_tx_bufs / rx_bufs: report what is still owned */
	for (q = 0; q < NQ; q++) {
		unsigned int i, tx = 0, rx = 0;
		for (i = 0; i < RING; i++) {
			tx += g.tx[q].slot[i].used;
			rx += g.rx[q].slot[i].used;
		}
		fprintf(mock_out, "RET down q %d tx_freed %u rx_freed %u\n", q, tx, rx);
	}
}

/* al_eth_set_rx_mode */
void g_set_rx_mode(int promisc, int allmulti, int mc_count, int uc_count)
{
	int i;

	if (promisc) {
		mac_table_promiscuous_set(1, 1);
		return;
	}
	if (allmulti)
		mac_table_all_multicast_add(MAC_TABLE_ALL_MC, 1);
	else if (mc_count == 0)
		mac_table_entry_clear(MAC_TABLE_ALL_MC);
	else
		mac_table_all_multicast_add(MAC_TABLE_ALL_MC, 1);
	if (uc_count) {
		if (uc_count > MAC_TABLE_UC_MAX) {
			mac_table_promiscuous_set(1, 1);
			return;
		}
		for (i = MAC_TABLE_UC_BASE + 1; i < MAC_TABLE_UC_BASE + MAC_TABLE_UC_MAX; i++)
			mac_table_entry_clear(i);
		for (i = 0; i < uc_count; i++)
			mac_table_unicast_add(MAC_TABLE_UC_BASE + 1 + i, 1);
	}
	mac_table_promiscuous_set(0, 1);
}

/* al_eth_set_mac_addr (is_valid_ether_addr checked by the caller) */
void g_set_mac(const uint8_t *a)
{
	memcpy(g.mac_addr, a, 6);
	mac_table_unicast_add(MAC_TABLE_UC_BASE, 1);
}

/* al_eth_get_stats64 */
void g_stats(void)
{
	struct al_eth_mac_stats s;

	if (!g.up) {
		fprintf(mock_out, "RET stats down\n");
		return;
	}
	al_eth_mac_stats_get(&g.hal, &s);
	fprintf(mock_out, "RET stats rx_packets %llu tx_packets %llu rx_bytes %llu tx_bytes %llu multicast %llu "
		"rx_length_errors %llu rx_crc_errors %llu rx_frame_errors %llu rx_fifo_errors %llu rx_errors %llu tx_errors %llu\n",
		(unsigned long long)s.aFramesReceivedOK, (unsigned long long)s.aFramesTransmittedOK,
		(unsigned long long)s.aOctetsReceivedOK, (unsigned long long)s.aOctetsTransmittedOK,
		(unsigned long long)s.ifInMulticastPkts,
		(unsigned long long)(uint32_t)(s.etherStatsUndersizePkts + s.etherStatsFragments + s.etherStatsJabbers + s.etherStatsOversizePkts),
		(unsigned long long)s.aFrameCheckSequenceErrors, (unsigned long long)s.aAlignmentErrors,
		(unsigned long long)s.etherStatsDropEvents, (unsigned long long)s.ifInErrors,
		(unsigned long long)s.ifOutErrors);
}

/* al_eth_is_mss_valid, verbatim (types included) */
static int al_eth_is_mss_valid(uint32_t mss, uint32_t total_payload_len, uint32_t header_len)
{
	int res_payload_len;

	res_payload_len = total_payload_len % mss;
	return (((res_payload_len) == 0) || ((header_len + res_payload_len) >= 60));
}

/* al_eth_tx_tso_mss_fix, verbatim; gso_size is the skb's unsigned short */
static int tso_mss_fix(const struct gskb *sk, uint32_t orig_mss, uint16_t *gso_size)
{
	int header_len, total_payload_len;
	int new_mss = orig_mss;
	int last_frame_pad;
	int i;

	header_len = sk->transport_off + sk->doff * 4;
	total_payload_len = sk->len - header_len;
	if (al_eth_is_mss_valid(orig_mss, total_payload_len, header_len))
		return 0;
	if (orig_mss < 64)
		return -EINVAL;
	last_frame_pad = 60 - header_len;
	if (orig_mss >= 260) {
		if ((total_payload_len / orig_mss) < last_frame_pad)
			new_mss = orig_mss - last_frame_pad;
		else
			new_mss = orig_mss - 1;
	} else {
		for (i = 1; i <= last_frame_pad; i++)
			if (al_eth_is_mss_valid(orig_mss - i, total_payload_len, header_len)) {
				new_mss = orig_mss - i;
				break;
			}
	}
	*gso_size = new_mss;
	return 0;
}

/* al_eth_start_xmit (+ al_eth_tx_csum); hal_pkt lives in the tx_info slot and keeps
 * whatever a previous packet left in fields this packet does not set */
void g_xmit(int q, struct gskb *sk)
{
	struct gtx *t = &g.tx[q];
	uint16_t ntu = t->ntu;
	struct gtxslot *s = &t->slot[ntu];
	struct al_eth_pkt *pkt = &s->pkt;
	uint32_t mss = sk->gso_size, l4 = 0;
	uint16_t gso = sk->gso_size;
	int i, descs;

	if (sk->len < 60) {           /* eth_skb_pad: linear, padded */
		sk->len = 60;
		sk->headlen = 60;
		sk->nr_frags = 0;
	}
	s->used = 1;
	pkt->flags = AL_ETH_TX_FLAGS_INT;
	if (sk->partial || mss) {
		struct al_eth_meta_data *m = &t->meta;
		if (mss)
			pkt->flags |= AL_ETH_TX_FLAGS_TSO | AL_ETH_TX_FLAGS_L4_CSUM;
		else
			pkt->flags |= AL_ETH_TX_FLAGS_L4_CSUM | AL_ETH_TX_FLAGS_L4_PARTIAL_CSUM;
		switch (sk->ip_version) {
		case 4:
			pkt->l3_proto_idx = AL_ETH_PROTO_ID_IPv4;
			if (mss)
				pkt->flags |= AL_ETH_TX_FLAGS_IPV4_L3_CSUM;
			l4 = sk->l4proto;
			break;
		case 6:
			pkt->l3_proto_idx = AL_ETH_PROTO_ID_IPv6;
			l4 = sk->l4proto;
			break;
		default:
			break;
		}
		if (l4 == 6) {
			pkt->l4_proto_idx = AL_ETH_PROTO_ID_TCP;
			if (mss && tso_mss_fix(sk, mss, &gso)) {
				s->used = 0;
				fprintf(mock_out, "RET xmit drop mss\n");
				return;
			}
		} else if (l4 == 17)
			pkt->l4_proto_idx = AL_ETH_PROTO_ID_UDP;
		pkt->source_vlan_count = sk->vlan ? 1 : 0;
		m->words_valid = 4;
		m->l3_header_len = sk->nh_len;
		m->l3_header_offset = sk->nh_off - 4 * pkt->source_vlan_count;
		m->l4_header_len = sk->doff;
		m->mss_idx_sel = 0;
		m->mss_val = gso;
		pkt->meta = m;
	} else
		pkt->meta = NULL;
	pkt->bufs[0].addr = sk->addr[0];
	pkt->bufs[0].len = sk->headlen;
	for (i = 0; i < sk->nr_frags; i++) {
		pkt->bufs[i + 1].addr = sk->addr[i + 1];
		pkt->bufs[i + 1].len = sk->frag_len[i];
	}
	pkt->num_of_bufs = 1 + sk->nr_frags;
	t->ntu = (ntu + 1) & (RING - 1);
	descs = al_eth_tx_pkt_prepare(g.txq[q], pkt);
	s->descs = descs;
	s->len = sk->len;
	if (al_udma_available_get(g.txq[q]) < MAX_SKB_FRAGS + 2)
		t->stopped = 1;
	al_eth_tx_dma_action(g.txq[q], descs);
	fprintf(mock_out, "RET xmit q %d descs %d gso %u stopped %d\n", q, descs, gso, t->stopped);
}

/* al_eth_tx_poll */
void g_tx_poll(int q)
{
	struct gtx *t = &g.tx[q];
	int done = al_eth_comp_tx_get(g.txq[q]);
	unsigned int pkts = 0, bytes = 0;
	uint16_t ntc = t->ntc;

	while (done) {
		struct gtxslot *s = &t->slot[ntc];
		if (s->descs > done)
			break;
		s->used = 0;
		bytes += s->len;
		pkts++;
		done -= s->descs;
		ntc = (ntc + 1) & (RING - 1);
	}
	t->ntc = ntc;
	if (t->stopped && al_udma_available_get(g.txq[q]) > TX_WAKEUP_THRESH)
		t->stopped = 0;
	al_reg_write32_relaxed((uint8_t *)g.udma + g.tx_unmask_off, ~(1u << q));
	fprintf(mock_out, "RET tx_poll q %d pkts %u bytes %u stopped %d\n", q, pkts, bytes, t->stopped);
}

/* al_eth_update_intr_moderation, Rx */
static void update_moderation(int q)
{
	struct grx *r = &g.rx[q];
	unsigned int cur = r->mod_idx, nw = cur, sm;

	if (!r->packets || !r->bytes)
		return;
	if (cur == 0) {
		if (r->packets > g.modtab[cur].pkts || r->bytes > g.modtab[cur].bytes)
			nw = cur + 1;
	} else {
		if (r->packets <= g.modtab[cur - 1].pkts || r->bytes <= g.modtab[cur - 1].bytes)
			nw = cur - 1;
		else if (r->packets > g.modtab[cur].pkts || r->bytes > g.modtab[cur].bytes) {
			if (cur != 4)
				nw = cur + 1;
		}
	}
	r->packets = 0;
	r->bytes = 0;
	sm = (g.modtab[nw].interval * 4 + 6 * r->smoothed + 5) / 10;
	r->smoothed = sm;
	r->mod_idx = nw;
	al_iofic_msix_moder_interval_config(&((struct unit_regs *)g.udma)->gen.interrupt_regs.main_iofic,
					    AL_INT_GROUP_B, q, sm);
}

/* al_eth_rx_poll (+ al_eth_rx_skb, al_eth_rx_checksum, the RXHASH part); features: RXCSUM, RXHASH on */
int g_rx_poll(int q, int budget)
{
	struct grx *r = &g.rx[q];
	struct al_eth_pkt pkt;
	int work = 0;
	uint16_t ntc = r->ntc;
	unsigned int need, got;

	memset(&pkt, 0, sizeof(pkt));
	do {
		unsigned int descs = al_eth_pkt_rx(g.rxq[q], &pkt), i, len = 0;
		int csum_ok = 0, l4hash = 0;

		if (descs == 0)
			break;
		r->packets++;
		if (pkt.flags & (AL_ETH_RX_ERROR | AL_UDMA_CDESC_ERROR)) {
			ntc = (ntc + descs) & (RING - 1);
			fprintf(mock_out, "RET rx q %d error flags %08x descs %u\n", q, pkt.flags, descs);
			goto next;
		}
		if (pkt.bufs[0].len <= SMALL_COPY_LEN) {
			len = pkt.bufs[0].len;
			ntc = (ntc + 1) & (RING - 1);
			r->bytes += len;
		} else {
			for (i = 0; i < descs; i++) {
				r->slot[ntc].used = 0;
				len += pkt.bufs[i].len;
				r->bytes += pkt.bufs[i].len;
				ntc = (ntc + 1) & (RING - 1);
			}
		}
		if (!((pkt.l3_proto_idx == AL_ETH_PROTO_ID_IPv4) && (pkt.flags & AL_ETH_RX_FLAGS_L3_CSUM_ERR)) &&
		    ((pkt.l4_proto_idx == AL_ETH_PROTO_ID_TCP) || (pkt.l4_proto_idx == AL_ETH_PROTO_ID_UDP)) &&
		    !(pkt.flags & AL_ETH_RX_FLAGS_L4_CSUM_ERR))
			csum_ok = 1;
		l4hash = (pkt.l4_proto_idx == AL_ETH_PROTO_ID_TCP) || (pkt.l4_proto_idx == AL_ETH_PROTO_ID_UDP);
		fprintf(mock_out, "RET rx q %d descs %u len %u small %d csum %d hash %08x l4 %d\n", q, descs, len,
			pkt.bufs[0].len <= SMALL_COPY_LEN, csum_ok,
			(((uint32_t)pkt.rxhash) << 16) ^ pkt.rxhash, l4hash);
next:
		budget--;
		work++;
	} while (budget);
	r->ntc = ntc;
	need = al_udma_available_get(g.rxq[q]);
	got = refill_rx(q, need);
	if (got < need)
		fprintf(mock_out, "RET rx_poll q %d resched\n", q);
	else if (budget > 0) {
		if (g.adaptive)
			update_moderation(q);
		al_reg_write32_relaxed((uint8_t *)g.udma + g.rx_unmask_off, ~(1u << q));
	}
	fprintf(mock_out, "RET rx_poll q %d work %d\n", q, work);
	return work;
}

/* al_eth_intr_msix_mgmt */
void g_mgmt_irq(void)
{
	uint32_t a = al_udma_iofic_read_cause(g.udma, AL_UDMA_IOFIC_LEVEL_PRIMARY, AL_INT_GROUP_D);
	uint32_t b = al_udma_iofic_read_cause(g.udma, AL_UDMA_IOFIC_LEVEL_SECONDARY, AL_INT_GROUP_A);
	uint32_t c = al_udma_iofic_read_cause(g.udma, AL_UDMA_IOFIC_LEVEL_SECONDARY, AL_INT_GROUP_B);
	fprintf(mock_out, "RET mgmt %08x %08x %08x\n", a, b, c);
}

/* al_eth_link_status_get (CS8210 port 3 polls it; RGMII/SGMII for completeness) */
void g_link_status(void)
{
	struct al_eth_link_status st;
	int rc;

	memset(&st, 0, sizeof(st));
	rc = al_eth_link_status_get(&g.hal, &st);
	fprintf(mock_out, "RET link rc %d up %d\n", rc, st.link_up);
}

/* the HAL's MAC configuration on its own, for every mode / link setting */
void g_mac_config(int mode, int force_1000x, int an, int speed, int duplex)
{
	int rc = al_eth_mac_config(&g.hal, mode);
	fprintf(mock_out, "RET mac_config %d\n", rc);
	rc = al_eth_mac_link_config(&g.hal, force_1000x, an, speed, duplex);
	fprintf(mock_out, "RET link_config %d\n", rc);
}
