/*
 * Host test of al_init_pcie_v3_rc.c (from the U-Boot patch) on the Alpine
 * V3 HAL copy of the same patch (verbatim al_hal_* files, AL_DEV_ID =
 * ALPINE_V3) against a register model of one rev-4 external port with four
 * AVG SerDes lanes:
 *  - memory-backed unit, PBS (chip id: Alpine V3 main) and lane register
 *    files; the LTSSM reads L0 at Gen3 x4 while the port is enabled in PBS
 *    and LTSSM_EN is set, unless the test holds the link down;
 *  - each lane's core-interrupt mailbox answers at once: 0 (firmware CRC
 *    valid) for the CRC check, 0 for everything else.
 *
 *  cold:  all registers zero (as after stage3) -> in the vendor
 *         al_init_pcie() order: port enable, disable, enable; per lane the
 *         firmware CRC/revision/build reads, then the 8 PCIe iCal
 *         configuration interrupts (interleaved across lanes, in sequence
 *         order), the PCS interrupts on; then link start; a Gen3 x4 link;
 *  warm:  link up, reinit -> link stop, disable first;
 *  started, no reinit -> "link already started", nothing written;
 *  stuck: no link -> error at "link up wait", link stopped, memories shut
 *         down, port disabled (the vendor teardown).
 */
#include <assert.h>
#include <stdlib.h>
#include <execinfo.h>
#include "al_init_pcie_v3_rc.h"
#include "al_hal_pbs_regs.h"
#include "al_hal_pcie_w_reg.h"
#include "al_hal_pcie_axi_reg.h"
#include "al_hal_unit_adapter_regs.h"
#include "al_hal_serdes_avg_internal.h"
#include "al_hal_serdes_avg_lane_regs.h"

#define UNIT_SIZE 0x20000
#define PBS_SIZE 0x1000
#define NLANES 4

static uint8_t *unit, *pbs, *lanes;
static uint32_t *info0, *port_init, *conf1;
static uint16_t *lnksta;
static int stuck;
static unsigned long us_total;
static unsigned int nwrites;
static char seq[512];
static size_t seqn;
static unsigned int ints[NLANES][32], nints[NLANES];

static struct al_srd_1lane_regs *lane(int i)
{
	return (struct al_srd_1lane_regs *)(lanes + i * 0x400);
}

static int in(const void *a, const uint8_t *base, size_t size)
{
	return (const uint8_t *)a >= base && (const uint8_t *)a < base + size;
}

static int valid(const void *a)
{
	if (!(in(a, unit, UNIT_SIZE) || in(a, pbs, PBS_SIZE) || in(a, lanes, NLANES * 0x400)))
		{ void *bt[8]; fprintf(stderr, "bad access %p (unit %p pbs %p lanes %p)\n", a, unit, pbs, lanes); backtrace_symbols_fd(bt, backtrace(bt, 8), 2); }
	return in(a, unit, UNIT_SIZE) || in(a, pbs, PBS_SIZE) || in(a, lanes, NLANES * 0x400);
}

static int link_up(void)
{
	return !stuck && (*conf1 & 1) && (*port_init & 1);
}

uint32_t model_r32(const void *a)
{
	assert(valid(a));
	if (a == info0)
		return link_up() ? (0x11 << PCIE_W_DEBUG_INFO_0_LTSSM_STATE_SHIFT) : 0;
	return *(const uint32_t *)a;
}

uint16_t model_r16(const void *a)
{
	assert(valid(a));
	if (a == lnksta)
		return link_up() ? 0x0043 : 0;	/* 8GT/s, x4 */
	return *(const uint16_t *)a;
}

uint8_t model_r8(const void *a)
{
	return *(const uint8_t *)a;
}

static void note(void *a, uint32_t old, uint32_t v)
{
	int i;

	if (a == conf1 && ((old ^ v) & 1))
		seq[seqn++] = v & 1 ? 'E' : 'D';	/* port enable / disable */
	if (a == port_init && ((old ^ v) & 1))
		seq[seqn++] = v & 1 ? 'S' : 's';	/* link start / stop */
	for (i = 0; i < NLANES; i++) {
		if (a == &lane(i)->gen.i_core_interrupt) {
			unsigned int code = (v & SRD_1LANE_GEN_I_CORE_INTERRUPT_CODE_MASK) >>
				SRD_1LANE_GEN_I_CORE_INTERRUPT_CODE_SHIFT;

			assert(nints[i] < 32);
			ints[i][nints[i]++] = code;
			seq[seqn++] = '0' + i;
			/* answer at once: not active, not in progress, data 0 */
			lane(i)->gen.i_core_interrupt_res = 0;
		}
		if (a == &lane(i)->pcie.cfg && (old & SRD_1LANE_PCIE_CFG_PCS_INTERRUPT_DISABLE) !=
		    (v & SRD_1LANE_PCIE_CFG_PCS_INTERRUPT_DISABLE))
			seq[seqn++] = v & SRD_1LANE_PCIE_CFG_PCS_INTERRUPT_DISABLE ? 'p' : 'P';
	}
	assert(seqn < sizeof(seq));
}

void model_w32(void *a, uint32_t v)
{
	assert(valid(a));
	note(a, *(uint32_t *)a, v);
	*(uint32_t *)a = v;
	nwrites++;
}

void model_w16(void *a, uint16_t v)
{
	assert(valid(a));
	*(uint16_t *)a = v;
	nwrites++;
}

void model_w8(void *a, uint8_t v)
{
	assert(valid(a));
	*(uint8_t *)a = v;
	nwrites++;
}

void model_udelay(unsigned long us)
{
	us_total += us;
}

static struct al_serdes_complex_obj cplx;
static struct al_serdes_grp_obj lobj[NLANES];
static struct al_serdes_grp_obj *lp[NLANES];
static uint8_t sbm[0x1000];

static void setup(int warm)
{
	struct al_pcie_port p;
	struct al_pbs_regs *pr;
	int i;

	free(unit);
	free(pbs);
	free(lanes);
	unit = calloc(1, UNIT_SIZE);
	pbs = calloc(1, PBS_SIZE);
	lanes = calloc(1, NLANES * 0x400);
	pr = (struct al_pbs_regs *)pbs;
	pr->unit.chip_id = PBS_UNIT_CHIP_ID_DEV_ID_ALPINE_V3 << PBS_UNIT_CHIP_ID_DEV_ID_SHIFT |
			   PBS_UNIT_CHIP_ID_DEV_REV_ID_ALPINE_V3_MAIN;
	/* AXI device_id: every Alpine V3 unit reads the x16 device id; subrev x4 */
	((struct al_pcie_revx_regs *)unit)->axi.device_id.device_rev_id =
		PCIE_AXI_DEVICE_ID_REG_DEV_ID_X16 | PCIE_AXI_DEVICE_ID_REG_REV_ID_X4;
	conf1 = &pr->unit.pcie_conf_1;
	stuck = 0;
	seqn = 0;
	memset(seq, 0, sizeof(seq));
	memset(nints, 0, sizeof(nints));
	us_total = 0;
	nwrites = 0;

	al_pcie_hal_failed = 0;
	assert(!al_pcie_port_handle_init(&p, unit, pbs, 0));
	assert(p.rev_id == AL_PCIE_REV_ID_4 && !al_pcie_hal_failed);
	info0 = p.regs->app.debug.info_0;
	port_init = p.regs->app.global_ctrl.port_init;
	lnksta = (uint16_t *)p.regs->core_space[0].pcie_cap_base + (AL_PCI_EXP_LNKSTA >> 1);

	al_serdes_avg_complex_handle_init(lanes, sbm, AL_SRDS_COMPLEX_TYPE_TR_MAIN,
					  NULL, NULL, &cplx);
	for (i = 0; i < NLANES; i++) {
		lane(i)->pcie.cfg = SRD_1LANE_PCIE_CFG_PCS_INTERRUPT_DISABLE;
		al_serdes_avg_handle_init(lane(i), &cplx, i, &lobj[i]);
		lp[i] = &lobj[i];
	}
	if (warm) {
		*conf1 |= 1;
		*port_init |= 1;
		assert(link_up());
	}
	seqn = 0;
	memset(seq, 0, sizeof(seq));
	nwrites = 0;
}

/* per lane: CRC check, revision x2, build id, then the 8 iCal codes */
static const unsigned int want_ints[] = {
	CORE_INT_CRC_CHECK_REG_NUM, CORE_INT_REVISION_GET_REG_NUM,
	CORE_INT_REVISION_GET_REG_NUM, CORE_INT_BUILD_ID_GET_REG_NUM,
	CORE_INT_PLL_CAL_CF_MAX0_PCIE_REG_NUM, CORE_INT_PLL_CAL_CF_MIN1_PCIE_REG_NUM,
	CORE_INT_PLL_CAL_CF_MAX2_PCIE_REG_NUM, CORE_INT_PLL_CAL_CF_MIN3_PCIE_REG_NUM,
	CORE_INT_SET_RX_EQ_REG_NUM, CORE_INT_SET_RX_EQ_REG_NUM, CORE_INT_SET_RX_EQ_REG_NUM,
	CORE_INT_SET_RX_EQ_REG_NUM,
};

static void check_lanes(const char *what, unsigned int n)
{
	unsigned int i, j;

	for (i = 0; i < n; i++) {
		if (nints[i] != sizeof(want_ints) / sizeof(want_ints[0])) {
			printf("%s: lane %u got %u core interrupts\n", what, i, nints[i]);
			abort();
		}
		for (j = 0; j < nints[i]; j++)
			assert(ints[i][j] == want_ints[j]);
		assert(!(lane(i)->pcie.cfg & SRD_1LANE_PCIE_CFG_PCS_INTERRUPT_DISABLE));
	}
	for (; i < NLANES; i++)
		assert(!nints[i]);
}

int main(void)
{
	struct al_init_pcie_v3_rc_cfg c = {
		.port_id = 0, .gen = 3, .lanes = 4, .wait_for_link_ms = 100,
		.serdes = lp, .serdes_num = NLANES,
	};
	struct al_init_pcie_v3_rc_result res;
	struct al_pcie_port port;
	const char *s;
	int rc;

	/* cold: straight from al-boot stage3 */
	setup(0);
	c.pcie_reg_base = unit;
	c.pbs_reg_base = pbs;
	rc = al_init_pcie_v3_rc(&port, &c, &res);
	printf("cold: rc %d, link %s gen%d x%d, torn down %d, %u writes, %lu us of delays\n   seq %s\n",
	       rc, res.status.link_up ? "up" : "down", res.status.speed, res.status.lanes,
	       res.torn_down, nwrites, us_total, seq);
	assert(!rc && res.status.link_up && res.status.speed == 3 && res.status.lanes == 4);
	assert(!res.torn_down && !al_pcie_hal_failed);
	/* E D E, then lane work (digits), then the four PCS enables, then S */
	assert(!strncmp(seq, "EDE", 3) && seq[strlen(seq) - 1] == 'S');
	s = strstr(seq, "PPPP");
	assert(s && s + 5 == seq + strlen(seq));
	for (s = seq + 3; *s != 'P'; s++)
		assert(*s >= '0' && *s < '0' + NLANES);
	check_lanes("cold", 4);
	/* iCal round-robin: after the per-lane firmware reads, lanes alternate */
	assert(strstr(seq, "0123012301230123"));
	assert(*conf1 & 1);

	/* a narrower port (fallback x2): only its first two lanes are touched */
	setup(0);
	c.pcie_reg_base = unit;
	c.pbs_reg_base = pbs;
	c.lanes = 2;
	rc = al_init_pcie_v3_rc(&port, &c, &res);
	assert(!rc);
	check_lanes("x2", 2);
	c.lanes = 4;

	/* skip_serdes_ical: firmware reads and PCS enable only */
	setup(0);
	c.pcie_reg_base = unit;
	c.pbs_reg_base = pbs;
	c.skip_serdes_ical = AL_TRUE;
	rc = al_init_pcie_v3_rc(&port, &c, &res);
	assert(!rc && nints[0] == 4 && !(lane(0)->pcie.cfg & SRD_1LANE_PCIE_CFG_PCS_INTERRUPT_DISABLE));
	c.skip_serdes_ical = AL_FALSE;

	/* started, no reinit: refused before any write */
	setup(1);
	c.pcie_reg_base = unit;
	c.pbs_reg_base = pbs;
	rc = al_init_pcie_v3_rc(&port, &c, &res);
	printf("started: rc %d at '%s', %u writes\n", rc, res.step, nwrites);
	assert(rc && !strcmp(res.step, "link already started") && !nwrites && link_up());

	/* warm, reinit: link stop and disable first */
	setup(1);
	c.pcie_reg_base = unit;
	c.pbs_reg_base = pbs;
	c.reinit = AL_TRUE;
	rc = al_init_pcie_v3_rc(&port, &c, &res);
	printf("warm: rc %d, link %s, torn down %d, seq %s\n", rc,
	       res.status.link_up ? "up" : "down", res.torn_down, seq);
	assert(!rc && res.status.link_up && res.torn_down && !strncmp(seq, "sDEDE", 5));
	c.reinit = AL_FALSE;

	/* stuck: the link never trains */
	setup(0);
	c.pcie_reg_base = unit;
	c.pbs_reg_base = pbs;
	stuck = 1;
	rc = al_init_pcie_v3_rc(&port, &c, &res);
	printf("stuck: rc %d at '%s', seq ...%s, port %s, %lu us of delays\n", rc, res.step,
	       seq + (strlen(seq) > 8 ? strlen(seq) - 8 : 0), (*conf1 & 1) ? "enabled" : "disabled",
	       us_total);
	assert(rc && !strcmp(res.step, "link up wait"));
	assert(!(*conf1 & 1) && !(*port_init & 1));
	assert(seq[strlen(seq) - 2] == 's' && seq[strlen(seq) - 1] == 'D');
	assert(us_total >= 100000);		/* waited the full link-up timeout */

	printf("pcie-init (Alpine V3): ok (cold Gen3 x4 with SerDes iCal, x2, skip-iCal, "
	       "started, warm reinit, stuck link)\n");
	return 0;
}
