/* SPDX-License-Identifier: GPL-2.0 */
#ifndef DIFFTEST_GLUE_H
#define DIFFTEST_GLUE_H
#include "al_hal_eth.h"

struct gslot { int used; uint64_t addr; uint32_t len; };
struct gtxslot { int used; int descs; uint32_t len; struct al_eth_pkt pkt; };
struct gmod { unsigned int interval, pkts, bytes; };

struct gtx {
	struct al_udma_q_params qp;
	int ring;
	uint16_t ntu, ntc;
	int stopped;
	struct al_eth_meta_data meta;
	struct gtxslot slot[1024];
};

struct grx {
	struct al_udma_q_params qp;
	int ring, cring;
	uint16_t ntu, ntc;
	uint32_t allocs, attempts;
	struct gslot slot[1024];
	unsigned int mod_idx, smoothed, packets, bytes;
	unsigned int hw_idx, hw_ring_id;       /* the fake hardware's completion position */
};

/* what al_eth_start_xmit reads from an skb */
struct gskb {
	uint32_t len, headlen;
	int nr_frags;
	uint32_t frag_len[17];
	uint64_t addr[18];
	int partial;                 /* ip_summed == CHECKSUM_PARTIAL */
	uint32_t gso_size;
	int ip_version, l4proto;
	uint32_t nh_off, nh_len, transport_off, doff;
	int vlan;                    /* skb->protocol == 802.1Q */
};

struct glue {
	struct al_hal_eth_adapter hal;
	struct al_eth_adapter_params params;
	void *udma, *ec, *mac;
	int rev_id;
	uint8_t mac_addr[6];
	int mac_mode, phy_exist, an_en, autoneg, force_1000_base_x, duplex, phy_if;
	unsigned int speed, mdio_freq, serdes_lane, ref_clk_freq;
	int flow_ctrl_supported, flow_ctrl_active;
	int reset_requested, up, mtu, adaptive;
	unsigned int tx_usecs, rx_usecs;
	uint32_t hash_key[10];
	uint32_t rss_ind_tbl[256];
	struct gmod mod[5], modtab[5];
	uint32_t tx_unmask_off, rx_unmask_off;
	struct al_udma_q *txq[4], *rxq[4];
	struct gtx tx[4];
	struct grx rx[4];
};

extern struct glue g;
uint64_t rx_buf_addr(int q, uint32_t n);
int rx_alloc_fails(int q, uint32_t attempt);
int g_probe(int rev_id);
int g_up(void);
void g_down(void);
int g_change_mtu(int mtu);
void g_set_rx_mode(int promisc, int allmulti, int mc_count, int uc_count);
void g_set_mac(const uint8_t *a);
void g_stats(void);
void g_xmit(int q, struct gskb *sk);
void g_tx_poll(int q);
int g_rx_poll(int q, int budget);
void g_mgmt_irq(void);
void g_link_status(void);
void g_mac_config(int mode, int force_1000x, int an, int speed, int duplex);
void function_reset(void);
#endif
