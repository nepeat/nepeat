/*
 * Host test of al_init_pcie_rc.c (from the U-Boot patch) on Annapurna's
 * PCIe HAL (al-pcie-hal/, verbatim) against a register model of one rev-3
 * external port: memory-backed unit and PBS register files; the LTSSM
 * reads as L0 (gen1 x1) while the port is enabled in PBS and LTSSM_EN is
 * set, unless the test holds the link down.
 *
 *  cold:  all registers zero -> the init leaves the AXI words the factory
 *         U-Boot leaves (u01 dump, inventory/u01/spi-uboot-regs.txt) in
 *         the fields it owns, and a trained link;
 *  warm:  the factory state (dump words, port enabled, link up) -> the
 *         init stops the link and disables the port first, in the vendor
 *         order, and ends with the same dump words, bit for bit;
 *  stuck: the link never trains -> error at "link up wait", port left
 *         stopped, memories shut down and disabled (vendor teardown).
 */
#include <assert.h>
#include <stdlib.h>
#include "al_init_pcie_rc.h"
#include "al_hal_pbs_regs.h"
#include "al_hal_pcie_w_reg.h"
#include "al_hal_pcie_axi_reg.h"
#include "al_hal_unit_adapter_regs.h"

#define UNIT_SIZE 0x20000
#define PBS_SIZE 0x1000

static uint8_t *unit, *pbs;
static uint32_t *info0, *port_init;
static uint16_t *lnksta;
static uint32_t *conf1;		/* PBS pcie_conf_1: port enable bits */
static int stuck;
static unsigned long us_total;
static unsigned int nwrites;

/* write log of the two registers whose order matters */
static char seq[256];
static size_t seqn;

static int in(const void *a, const uint8_t *base, size_t size)
{
	return (const uint8_t *)a >= base && (const uint8_t *)a < base + size;
}

static int link_up(void)
{
	return !stuck && (*conf1 & 1) && (*port_init & 1);
}

uint32_t model_r32(const void *a)
{
	assert(in(a, unit, UNIT_SIZE) || in(a, pbs, PBS_SIZE));
	if (a == info0)
		return link_up() ? (0x11 << PCIE_W_DEBUG_INFO_0_LTSSM_STATE_SHIFT) : 0;
	return *(const uint32_t *)a;
}

uint16_t model_r16(const void *a)
{
	assert(in(a, unit, UNIT_SIZE) || in(a, pbs, PBS_SIZE));
	if (a == lnksta)
		return link_up() ? 0x0011 : 0;	/* 2.5GT/s, x1 */
	return *(const uint16_t *)a;
}

uint8_t model_r8(const void *a)
{
	return *(const uint8_t *)a;
}

static void note(const void *a, uint32_t old, uint32_t v)
{
	if (a == conf1 && ((old ^ v) & 1))
		seq[seqn++] = v & 1 ? 'E' : 'D';	/* port enable / disable */
	if (a == port_init && ((old ^ v) & 1))
		seq[seqn++] = v & 1 ? 'S' : 's';	/* link start / stop */
	assert(seqn < sizeof(seq));
}

void model_w32(void *a, uint32_t v)
{
	assert(in(a, unit, UNIT_SIZE) || in(a, pbs, PBS_SIZE));
	note(a, *(uint32_t *)a, v);
	*(uint32_t *)a = v;
	nwrites++;
}

void model_w16(void *a, uint16_t v)
{
	assert(in(a, unit, UNIT_SIZE) || in(a, pbs, PBS_SIZE));
	*(uint16_t *)a = v;
	nwrites++;
}

void model_w8(void *a, uint8_t v)
{
	*(uint8_t *)a = v;
	nwrites++;
}

void model_udelay(unsigned long us)
{
	us_total += us;
}

#define AXI(off) (*(uint32_t *)(unit + (off)))

/* u01, factory prompt, md.l 0xfd800000 (only words the init owns) */
static const struct { unsigned int off; uint32_t val, mask; } factory[] = {
	{ 0x00, 0x0014100c, 0x003c0000 },	/* axi parity int: bits 21:18 */
	{ 0x14, 0x0c0000bf, 0x0c000000 },	/* snoop override + enable */
	{ 0x18, 0x0c0000bf, 0x0c000000 },
	{ 0x20, 0x80000000, 0x80000000 },	/* port_config_live */
	{ 0x30, 0x000000ff, 0x000000ff },	/* cfg target bus mask (RC) */
	{ 0xc8, 0x00000014, 0xffffffff },	/* RC, 1 lane, memories on */
	{ 0xe0, 0x000000c8, 0x000004e0 },	/* ordering/snoop defaults */
	{ 0xe8, 0x2fa0200f, 0x2000ff00 },	/* hdr credits + bit 29 */
	{ 0xec, 0x02844c3c, 0xffffffff },	/* IB credits / OB reads */
};
static const uint32_t dump[64] = {
	0x0014100c, 0xbad0add1, 0x00000000, 0x00000000, 0x00000001, 0x0c0000bf, 0x0c0000bf, 0x00000000,
	0x80000000, 0x00000000, 0x00000001, 0x00000000, 0x000000ff, 0x00000000, 0x00000000, 0x00000000,
	0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0x00000fff, 0x00000fff, 0x00000fff, 0x0000ffff, 0, 0,
	0x0000ffff, 0xbad0add1, 0xbad0add1, 0xbad0add1, 0, 0, 0, 0x00000014,
	0, 0, 0, 0, 0, 0, 0, 0,
	0, 0, 0x00000014, 0xbad0add1, 0, 0, 0, 0,
	0x000000c8, 0x0001e848, 0x2fa0200f, 0x02844c3c, 0xbad0add1, 0xbad0add1, 0xbad0add1, 0xbad0add1,
};

static void setup(int warm)
{
	struct al_pcie_port p;
	struct al_pbs_regs *pr;

	free(unit);
	free(pbs);
	unit = calloc(1, UNIT_SIZE);
	pbs = calloc(1, PBS_SIZE);
	pr = (struct al_pbs_regs *)pbs;
	pr->unit.chip_id = PBS_UNIT_CHIP_ID_DEV_ID_ALPINE_V2 << PBS_UNIT_CHIP_ID_DEV_ID_SHIFT;
	AXI(0x16c) = 0x00020000;		/* device id: x8 controller = rev 3 */
	conf1 = &pr->unit.pcie_conf_1;
	stuck = 0;
	seqn = 0;
	memset(seq, 0, sizeof(seq));
	us_total = 0;
	nwrites = 0;

	assert(!al_pcie_port_handle_init(&p, unit, pbs, 0));
	assert(p.rev_id == AL_PCIE_REV_ID_3);
	info0 = p.regs->app.debug.info_0;
	port_init = p.regs->app.global_ctrl.port_init;
	lnksta = (uint16_t *)p.regs->core_space[0].pcie_cap_base + (AL_PCI_EXP_LNKSTA >> 1);

	if (warm) {
		memcpy(unit, dump, sizeof(dump));
		AXI(0x16c) = 0x00020000;
		*conf1 |= 1;
		*port_init |= 1;
		assert(link_up());
	}
}

static const struct al_init_pcie_rc_cfg cfg = {
	.port_id = 0, .gen = 1, .lanes = 1, .wait_for_link_ms = 1000,
};

static int check_factory(const char *what, int whole)
{
	int bad = 0;
	size_t i;

	for (i = 0; i < sizeof(factory) / sizeof(factory[0]); i++) {
		uint32_t m = whole ? 0xffffffff : factory[i].mask;
		uint32_t got = AXI(factory[i].off);

		if ((got & m) != (factory[i].val & m)) {
			printf("%s: AXI +%#04x = %08x, factory %08x (mask %08x)\n", what,
			       factory[i].off, got, factory[i].val, m);
			bad++;
		}
	}
	return bad;
}

int main(void)
{
	struct al_init_pcie_rc_cfg c = cfg;
	struct al_init_pcie_rc_result res;
	struct al_pcie_port port;
	int rc, bad = 0;

	/* cold: as straight from al-boot stage3 */
	setup(0);
	c.pcie_reg_base = unit;
	c.pbs_reg_base = pbs;
	rc = al_init_pcie_rc(&port, &c, &res);
	printf("cold: rc %d, link %s gen%d x%d, torn down %d, seq %s, %u writes, %lu us of delays\n",
	       rc, res.status.link_up ? "up" : "down", res.status.speed, res.status.lanes,
	       res.torn_down, seq, nwrites, us_total);
	assert(!rc && res.status.link_up && res.status.speed == 1 && res.status.lanes == 1);
	assert(!res.torn_down && !strcmp(seq, "EDES"));
	bad += check_factory("cold", 0);
	assert(*conf1 & 1);

	/* warm: the factory U-Boot's state, link up */
	setup(1);
	c.pcie_reg_base = unit;
	c.pbs_reg_base = pbs;
	rc = al_init_pcie_rc(&port, &c, &res);
	printf("warm: rc %d, link %s gen%d x%d, torn down %d, seq %s, %u writes, %lu us of delays\n",
	       rc, res.status.link_up ? "up" : "down", res.status.speed, res.status.lanes,
	       res.torn_down, seq, nwrites, us_total);
	/* vendor order: stop link, (shutdown) disable, enable, disable, enable, start */
	assert(!rc && res.status.link_up && res.torn_down && !strcmp(seq, "sDEDES"));
	bad += check_factory("warm", 1);

	/* stuck: the link never trains */
	setup(0);
	stuck = 1;
	c.pcie_reg_base = unit;
	c.pbs_reg_base = pbs;
	rc = al_init_pcie_rc(&port, &c, &res);
	printf("stuck: rc %d at '%s', seq %s, port %s, AXI +0xc8 %08x, %lu us of delays\n",
	       rc, res.step, seq, (*conf1 & 1) ? "enabled" : "disabled", AXI(0xc8), us_total);
	assert(rc && !strcmp(res.step, "link up wait"));
	assert(!(*conf1 & 1) && !(*port_init & 1));
	assert(AXI(0xc8) & PCIE_REV3_4_AXI_MISC_PCIE_GLOBAL_CONF_MEM_SHUTDOWN);
	assert(us_total >= 1000000);		/* waited the full second */

	if (bad) {
		printf("FAIL: %d register(s) differ from the factory state\n", bad);
		return 1;
	}
	printf("pcie-init: ok (cold, warm and stuck-link paths; factory AXI words reproduced)\n");
	return 0;
}
