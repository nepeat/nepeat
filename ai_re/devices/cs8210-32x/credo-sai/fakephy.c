/*
 * Step-0 harness for Credo's x86_64 SAI 1.2.12 (libsaicredo + owl):
 * create one SAI PHY "switch" whose REGISTER_READ/WRITE callbacks hit a
 * fake, logging Owl 800 slice instead of hardware. The deliverable is the
 * register access sequence. Runs only inside bwrap (see run-step0.sh):
 * no network, no /sys, no hardware.
 *
 * Log lines on stdout:  R|W  t_us  ctx  addr  reg  value
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include <saitypes.h>
#include <saistatus.h>
#include <saiswitch.h>
#include <saiport.h>
#include <saimacsec.h>

/* From sai.h, which drags in every SAI header; only these are needed. */
typedef enum { SAI_API_SWITCH_ = 1, SAI_API_PORT_ = 2, SAI_API_MACSEC_ = 43 } api_t;
typedef const char *(*profile_get_value_fn)(sai_switch_profile_id_t, const char *);
typedef int (*profile_get_next_value_fn)(sai_switch_profile_id_t, const char **, const char **);
typedef struct { profile_get_value_fn get; profile_get_next_value_fn next; } services_t;
typedef sai_status_t (*api_initialize_fn)(uint64_t, const services_t *);
typedef sai_status_t (*api_query_fn)(int, void **);
typedef sai_status_t (*log_set_fn)(int, int);

static struct timespec t0;
static FILE *logf;                      /* stdout, or stderr with LOG_STDERR (merged with libsai's log) */
#define printf(...) fprintf(logf, __VA_ARGS__)
static unsigned long naccess, maxaccess = 200000;
static uint32_t regs[0x10000];          /* SDK register space is 16-bit */
static unsigned char seeded[0x10000];

static unsigned long now_us(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return (t.tv_sec - t0.tv_sec) * 1000000UL + (t.tv_nsec - t0.tv_nsec) / 1000;
}

static void count(void)
{
	if (++naccess >= maxaccess) {
		printf("# access cap %lu reached, exiting\n", maxaccess);
		fflush(logf);
		_exit(3);
	}
}

static sai_status_t reg_read(uint64_t ctx, uint32_t addr, uint32_t reg, uint32_t n, uint32_t *val)
{
	for (uint32_t i = 0; i < n; i++) {
		uint32_t r = (reg + i) & 0xffff;
		val[i] = regs[r];
		printf("R %lu %lu %u %04x %04x%s\n", now_us(), (unsigned long)ctx, addr, r, val[i],
		       seeded[r] ? " seed" : "");
		count();
	}
	return SAI_STATUS_SUCCESS;
}

/*
 * Firmware mailbox, as seen in the vendor bring-up trace: arguments in
 * b723/b71c, command written to b71b; the MCU answers by rewriting b71b
 * (0xC0xy -> 0x0C00-style code) and leaves result data in b71c. Answers
 * are replayed from mbox.txt (trace2fake.py); unknown commands get the
 * generic (cmd >> 4) & 0x0f00 code and are logged as misses.
 */
#define MBOX_MAX 512
static struct { uint32_t cmd, a723, a71c, resp; int data, extra, d723; } mbox[MBOX_MAX];
static int nmbox;

static void mbox_answer(uint32_t cmd)
{
	for (int i = 0; i < nmbox; i++)
		if (mbox[i].cmd == cmd && mbox[i].a723 == regs[0xb723] && mbox[i].a71c == regs[0xb71c]) {
			regs[0xb71b] = mbox[i].resp;
			if (mbox[i].data >= 0)
				regs[0xb71c] = mbox[i].data;
			if (mbox[i].d723 >= 0)
				regs[0xb723] = mbox[i].d723;
			printf("# mbox %s cmd %04x a723 %04x a71c %04x -> %04x data %04x\n",
			       mbox[i].extra ? "EXTRA-hit" : "hit", cmd,
			       mbox[i].a723, mbox[i].a71c, regs[0xb71b], regs[0xb71c]);
			return;
		}
	for (int i = 0; i < nmbox; i++)       /* same command, other args */
		if (mbox[i].cmd == cmd) {
			regs[0xb71b] = mbox[i].resp;
			if (mbox[i].data >= 0)    /* same command's data from the trace */
				regs[0xb71c] = mbox[i].data;
			printf("# mbox args-miss cmd %04x a723 %04x a71c %04x -> %04x data %04x\n", cmd,
			       regs[0xb723], regs[0xb71c], regs[0xb71b], regs[0xb71c]);
			return;
		}
	/* Not in the trace: answer the generic "done, no data" code (class
	 * (cmd >> 4) & 0xf00, which libowl treats as success; 0x0300 = error).
	 * This IS an invented answer; every one is logged as MISS. */
	regs[0xb71b] = (cmd >> 4) & 0x0f00;
	printf("# mbox MISS (generic ok, not from trace) cmd %04x a723 %04x a71c %04x -> %04x\n", cmd, regs[0xb723], regs[0xb71c], regs[0xb71b]);
}

/*
 * MCU TCM model (FAKE_TCM, from trace2fake.py tcm.txt): a TCM read is
 * address -> b8fe:b8fc, status b8fa, value b8f6:b8f8; a TCM write is mailbox
 * 5040 with address b900 | (b901 & 0xff00) << 9 and value b723:b71c.
 * Lets libowl's read-modify-writes see the post-vendor TCM values.
 */
#define TCM_MAX 4096
static struct { uint32_t addr, val; } tcm[TCM_MAX];
static int ntcm;

static int tcm_find(uint32_t addr)
{
	for (int i = 0; i < ntcm; i++)
		if (tcm[i].addr == addr)
			return i;
	return -1;
}

static void tcm_read_port(void)
{
	uint32_t addr = regs[0xb8fe] << 16 | regs[0xb8fc];
	int i = tcm_find(addr);
	if (!ntcm)
		return;
	regs[0xb8fa] = 1;
	regs[0xb8f6] = i < 0 ? 0 : tcm[i].val >> 16;
	regs[0xb8f8] = i < 0 ? 0 : tcm[i].val & 0xffff;
	if (i < 0)
		printf("# tcm R %08x MISS (not in trace, reads 0)\n", addr);
	else
		printf("# tcm R %08x %08x\n", addr, tcm[i].val);
}

static void tcm_write_cmd(void)
{
	uint32_t addr = regs[0xb900] | (regs[0xb901] & 0xff00) << 9;
	uint32_t val = regs[0xb723] << 16 | regs[0xb71c];
	int i = tcm_find(addr);
	if (!ntcm)
		return;
	printf("# tcm W %08x %08x vendor %s", addr, val, i < 0 ? "--------" : "");
	if (i >= 0)
		printf("%08x", tcm[i].val);
	printf("\n");
	if (i < 0 && ntcm < TCM_MAX)
		i = ntcm++;
	if (i >= 0) {
		tcm[i].addr = addr;
		tcm[i].val = val;
	}
}

/*
 * FAKE_GATE=1: prototype of the write gate the real MDIO bridge must have for
 * an attach to a live slice. Only the mailbox (b71b/b71c/b723), the TCM read
 * address (b8fc/b8fe) and the TCM write block (b900-b903) may be written, and
 * a TCM write (mailbox 5040) must not change the word. Anything else kills
 * the process BEFORE the write is applied (exit 4), so libsai can never fall
 * through to ColdBoot (REG_CHIP_RST pulse, lane re-init) on real hardware.
 */
static int gate;

static int gate_ok(uint32_t r, uint32_t v)
{
	if (r == 0xb71c || r == 0xb723 || r == 0xb8fc || r == 0xb8fe || (r >= 0xb900 && r <= 0xb903))
		return 1;
	if (r != 0xb71b)
		return 0;
	if (v == 0x5040) {          /* TCM write: only if it does not change the word */
		uint32_t a = regs[0xb900] | (regs[0xb901] & 0xff00) << 9;
		int i = tcm_find(a);
		return i >= 0 && tcm[i].val == (regs[0xb723] << 16 | regs[0xb71c]);
	}
	/* query commands seen on the warm-attach path (I: read-only by name):
	 * b000 fw status, b008 fw register read, f000-f003 hash/crc/date/version,
	 * f005/f006 feature, a010-a01b port query. Everything else (e011, e021,
	 * 809f port config, 7060, ...) is refused. */
	return v == 0xb000 || v == 0xb008 || (v >= 0xf000 && v <= 0xf006) || (v >= 0xa010 && v <= 0xa01b);
}

static sai_status_t reg_write(uint64_t ctx, uint32_t addr, uint32_t reg, uint32_t n, const uint32_t *val)
{
	for (uint32_t i = 0; i < n; i++) {
		uint32_t r = (reg + i) & 0xffff;
		if (gate && !gate_ok(r, val[i])) {
			printf("# GATE blocked W %04x %04x (was %04x) after %lu accesses, exiting\n", r, val[i], regs[r], naccess);
			fflush(logf);
			_exit(4);
		}
		regs[r] = val[i];
		printf("W %lu %lu %u %04x %04x\n", now_us(), (unsigned long)ctx, addr, r, val[i]);
		count();
		if (r == 0xb71b && val[i] == 0x5040)
			tcm_write_cmd();
		if (r == 0xb71b && nmbox)
			mbox_answer(val[i]);
		if (r == 0xb8fc || r == 0xb8fe)
			tcm_read_port();
	}
	return SAI_STATUS_SUCCESS;
}

static void load_files(void)
{
	const char *p = getenv("FAKE_SEEDS");
	unsigned r, v, a, b, c, d;
	char ds[16];
	FILE *f;

	if (p && (f = fopen(p, "r"))) {
		int k = 0;
		while (fscanf(f, "%x %x", &r, &v) == 2) {
			regs[r & 0xffff] = v;
			seeded[r & 0xffff] = 1;
			k++;
		}
		fclose(f);
		printf("# %d seeded registers from %s\n", k, p);
	}
	p = getenv("FAKE_TCM");
	if (p && (f = fopen(p, "r"))) {
		char how[8];
		while (ntcm < TCM_MAX && fscanf(f, "%x %x %7s", &a, &b, how) == 3) {
			tcm[ntcm].addr = a;
			tcm[ntcm].val = b;
			ntcm++;
		}
		fclose(f);
		printf("# %d TCM words from %s\n", ntcm, p);
	}
	/* FAKE_MBOX: replayed from the vendor trace. FAKE_MBOX_EXTRA: answers NOT
	 * seen in the trace (justified in mbox-extra.txt); hits are logged as EXTRA. */
	const char *mb[2] = { getenv("FAKE_MBOX"), getenv("FAKE_MBOX_EXTRA") };
	for (int e = 0; e < 2; e++) {
		int n0 = nmbox;
		char line[256], d7[16];
		if (!mb[e] || !(f = fopen(mb[e], "r")))
			continue;
		while (nmbox < MBOX_MAX && fgets(line, sizeof line, f)) {
			/* cmd a723 a71c resp data(b71c|-) count [b723 answer] */
			int nf = sscanf(line, "%x %x %x %x %15s %u %15s", &a, &b, &c, &d, ds, &v, d7);
			if (line[0] == '#' || nf < 6)
				continue;
			mbox[nmbox].d723 = nf == 7 && d7[0] != '-' ? (int)strtoul(d7, NULL, 16) : -1;
			mbox[nmbox].cmd = a; mbox[nmbox].a723 = b; mbox[nmbox].a71c = c;
			mbox[nmbox].resp = d;
			mbox[nmbox].data = ds[0] == '-' ? -1 : (int)strtoul(ds, NULL, 16);
			mbox[nmbox].extra = e;
			nmbox++;
		}
		fclose(f);
		printf("# %d mailbox answers from %s\n", nmbox - n0, mb[e]);
	}
}

/* Profile keys come from the environment of this process: PROFILE_<key>. */
static const char *profile_get(sai_switch_profile_id_t id, const char *key)
{
	char name[160];
	snprintf(name, sizeof name, "PROFILE_%s", key);
	const char *v = getenv(name);
	printf("# profile_get_value(%u, %s) -> %s\n", id, key, v ? v : "(null)");
	return v;
}

static int profile_next(sai_switch_profile_id_t id, const char **k, const char **v)
{
	(void)id; (void)k; (void)v;
	return -1;
}

static void seed(uint32_t r, uint32_t v) { regs[r] = v; seeded[r] = 1; }

/* ---- FAKE_QUERY: read-mostly look at ports and MACsec (step 2, goal 4) ---- */
static const char *stname(sai_status_t st) { static char b[16]; snprintf(b, sizeof b, "%d", st); return b; }

static void get_switch_u32(sai_switch_api_t *sw, sai_object_id_t oid, sai_attr_id_t id, const char *name)
{
	sai_attribute_t x = { .id = id };
	sai_status_t st = sw->get_switch_attribute(oid, 1, &x);
	printf("# get switch %s -> %s value %u\n", name, stname(st), x.value.u32);
}

static void query_objects(api_query_fn query, sai_switch_api_t *sw, sai_object_id_t oid)
{
	sai_port_api_t *pa = NULL;
	sai_macsec_api_t *ma = NULL;
	sai_object_id_t ports[64];
	sai_attribute_t x;
	sai_status_t st;

	printf("# phase query_switch\n");
	get_switch_u32(sw, oid, SAI_SWITCH_ATTR_PORT_NUMBER, "PORT_NUMBER");
	get_switch_u32(sw, oid, SAI_SWITCH_ATTR_FIRMWARE_MAJOR_VERSION, "FIRMWARE_MAJOR_VERSION");
	get_switch_u32(sw, oid, SAI_SWITCH_ATTR_FIRMWARE_MINOR_VERSION, "FIRMWARE_MINOR_VERSION");
	x.id = SAI_SWITCH_ATTR_FIRMWARE_STATUS;
	st = sw->get_switch_attribute(oid, 1, &x);
	printf("# get switch FIRMWARE_STATUS -> %s value %d\n", stname(st), x.value.booldata);
	x.id = SAI_SWITCH_ATTR_PORT_LIST;
	x.value.objlist.count = 64;
	x.value.objlist.list = ports;
	st = sw->get_switch_attribute(oid, 1, &x);
	uint32_t np = st == SAI_STATUS_SUCCESS ? x.value.objlist.count : 0;
	printf("# get switch PORT_LIST -> %s count %u\n", stname(st), np);

	printf("# phase query_ports\n");
	query(SAI_API_PORT_, (void **)&pa);
	for (uint32_t i = 0; pa && i < np && i < 64; i++) {
		uint32_t lanes[16];
		sai_attribute_t pq[4];
		memset(pq, 0, sizeof pq);
		pq[0].id = SAI_PORT_ATTR_HW_LANE_LIST; pq[0].value.u32list.count = 16; pq[0].value.u32list.list = lanes;
		pq[1].id = SAI_PORT_ATTR_SPEED;
		pq[2].id = SAI_PORT_ATTR_FEC_MODE;
		pq[3].id = SAI_PORT_ATTR_ADMIN_STATE;
		for (int k = 0; k < 4; k++) {
			st = pa->get_port_attribute(ports[i], 1, &pq[k]);
			if (k == 0) {
				printf("# port 0x%lx HW_LANE_LIST -> %s:", (unsigned long)ports[i], stname(st));
				for (uint32_t j = 0; st == SAI_STATUS_SUCCESS && j < pq[0].value.u32list.count; j++)
					printf(" %u", lanes[j]);
				printf("\n");
			} else
				printf("# port 0x%lx attr %d -> %s value %u\n", (unsigned long)ports[i], pq[k].id, stname(st), pq[k].value.u32);
		}
	}

	printf("# phase create_macsec\n");
	query(SAI_API_MACSEC_, (void **)&ma);
	if (!ma || !getenv("FAKE_MACSEC"))
		return;
	for (int dir = 0; dir < 2; dir++) {      /* SAI_MACSEC_DIRECTION_EGRESS = 0, INGRESS = 1 */
		sai_object_id_t m = 0;
		sai_attribute_t c = { .id = SAI_MACSEC_ATTR_DIRECTION, .value.s32 = dir };
		st = ma->create_macsec(&m, oid, 1, &c);
		printf("# create_macsec(direction %d) -> %s oid 0x%lx after %lu accesses\n", dir, stname(st), (unsigned long)m, naccess);
		if (st != SAI_STATUS_SUCCESS)
			continue;
		printf("# phase query_macsec\n");
		int32_t suites[16];
		sai_object_id_t mports[64];
		sai_attribute_t q = { .id = SAI_MACSEC_ATTR_SUPPORTED_CIPHER_SUITE_LIST };
		q.value.s32list.count = 16; q.value.s32list.list = suites;
		st = ma->get_macsec_attribute(m, 1, &q);
		printf("# macsec dir %d SUPPORTED_CIPHER_SUITE_LIST -> %s:", dir, stname(st));
		for (uint32_t j = 0; st == SAI_STATUS_SUCCESS && j < q.value.s32list.count; j++)
			printf(" %d", suites[j]);
		printf("\n");
		static const struct { sai_attr_id_t id; const char *n; char t; } qa[] = {
			{ SAI_MACSEC_ATTR_PN_32BIT_SUPPORTED, "PN_32BIT_SUPPORTED", 'b' },
			{ SAI_MACSEC_ATTR_XPN_64BIT_SUPPORTED, "XPN_64BIT_SUPPORTED", 'b' },
			{ SAI_MACSEC_ATTR_GCM_AES128_SUPPORTED, "GCM_AES128_SUPPORTED", 'b' },
			{ SAI_MACSEC_ATTR_GCM_AES256_SUPPORTED, "GCM_AES256_SUPPORTED", 'b' },
			{ SAI_MACSEC_ATTR_SCI_IN_INGRESS_MACSEC_ACL, "SCI_IN_INGRESS_MACSEC_ACL", 'b' },
			{ SAI_MACSEC_ATTR_SWITCHING_MODE_CUT_THROUGH_SUPPORTED, "CUT_THROUGH_SUPPORTED", 'b' },
			{ SAI_MACSEC_ATTR_SWITCHING_MODE_STORE_AND_FORWARD_SUPPORTED, "STORE_AND_FORWARD_SUPPORTED", 'b' },
			{ SAI_MACSEC_ATTR_STATS_MODE_READ_SUPPORTED, "STATS_MODE_READ_SUPPORTED", 'b' },
			{ SAI_MACSEC_ATTR_STATS_MODE_READ_CLEAR_SUPPORTED, "STATS_MODE_READ_CLEAR_SUPPORTED", 'b' },
			{ SAI_MACSEC_ATTR_WARM_BOOT_SUPPORTED, "WARM_BOOT_SUPPORTED", 'b' },
			{ SAI_MACSEC_ATTR_WARM_BOOT_ENABLE, "WARM_BOOT_ENABLE", 'b' },
			{ SAI_MACSEC_ATTR_PHYSICAL_BYPASS_ENABLE, "PHYSICAL_BYPASS_ENABLE", 'b' },
			{ SAI_MACSEC_ATTR_SYSTEM_SIDE_MTU, "SYSTEM_SIDE_MTU", 'h' },
			{ SAI_MACSEC_ATTR_MAX_VLAN_TAGS_PARSED, "MAX_VLAN_TAGS_PARSED", 'c' },
			{ SAI_MACSEC_ATTR_AVAILABLE_MACSEC_FLOW, "AVAILABLE_MACSEC_FLOW", 'u' },
			{ SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SC, "AVAILABLE_MACSEC_SC", 'u' },
			{ SAI_MACSEC_ATTR_AVAILABLE_MACSEC_SA, "AVAILABLE_MACSEC_SA", 'u' },
			{ SAI_MACSEC_ATTR_MAX_SECURE_ASSOCIATIONS_PER_SC, "MAX_SECURE_ASSOCIATIONS_PER_SC", 's' },
			{ SAI_MACSEC_ATTR_STATS_MODE, "STATS_MODE", 's' },
		};
		for (size_t k = 0; k < sizeof qa / sizeof qa[0]; k++) {
			sai_attribute_t y = { .id = qa[k].id };
			st = ma->get_macsec_attribute(m, 1, &y);
			printf("# macsec dir %d %s -> %s value %d\n", dir, qa[k].n, stname(st),
			       qa[k].t == 'b' ? y.value.booldata : qa[k].t == 'h' ? y.value.u16 :
			       qa[k].t == 'c' ? y.value.u8 : qa[k].t == 'u' ? (int)y.value.u32 : y.value.s32);
		}
		sai_attribute_t pl = { .id = SAI_MACSEC_ATTR_SUPPORTED_PORT_LIST };
		pl.value.objlist.count = 64; pl.value.objlist.list = mports;
		st = ma->get_macsec_attribute(m, 1, &pl);
		printf("# macsec dir %d SUPPORTED_PORT_LIST -> %s count %u\n", dir, stname(st),
		       st == SAI_STATUS_SUCCESS ? pl.value.objlist.count : 0);
		printf("# phase create_macsec\n");
	}
}

int main(int argc, char **argv)
{
	logf = getenv("LOG_STDERR") ? stderr : stdout;
	setvbuf(stdout, NULL, _IOLBF, 0);
	clock_gettime(CLOCK_MONOTONIC, &t0);
	gate = getenv("FAKE_GATE") != NULL;
	if (getenv("MAX_ACCESS"))
		maxaccess = strtoul(getenv("MAX_ACCESS"), NULL, 0);

	/* Identity seen on our hardware: 1.00cc = 0x2161, 1.00cb = 0x00a2, chip version 0x20. */
	seed(0xcc, 0x2161);
	seed(0xcb, 0x00a2);
	seed(0xca, 0x0000);
	seed(0xf0, 0x0020);
	load_files();
	for (int i = 1; i + 1 < argc; i += 2)       /* extra seeds: REG VAL pairs */
		seed(strtoul(argv[i], NULL, 0) & 0xffff, strtoul(argv[i + 1], NULL, 0));

	void *h = dlopen("libsai.so.1", RTLD_NOW | RTLD_GLOBAL);
	if (!h) { printf("# dlopen: %s\n", dlerror()); return 2; }
	api_initialize_fn init = (api_initialize_fn)dlsym(h, "sai_api_initialize");
	api_query_fn query = (api_query_fn)dlsym(h, "sai_api_query");
	log_set_fn log_set = (log_set_fn)dlsym(h, "sai_log_set");
	if (!init || !query) { printf("# dlsym failed\n"); return 2; }

	services_t svc = { profile_get, profile_next };
	printf("# sai_api_initialize -> %d\n", init(0, &svc));
	if (log_set)
		for (int api = 1; api < 120; api++)
			log_set(api, 0 /* DEBUG */);

	sai_switch_api_t *sw = NULL;
	printf("# sai_api_query(SWITCH) -> %d\n", query(SAI_API_SWITCH_, (void **)&sw));
	if (!sw || !sw->create_switch) return 2;

	sai_attribute_t a[16];
	int n = 0;
	memset(a, 0, sizeof a);
	a[n].id = SAI_SWITCH_ATTR_INIT_SWITCH;        a[n++].value.booldata = true;
	a[n].id = SAI_SWITCH_ATTR_TYPE;               a[n++].value.s32 = SAI_SWITCH_TYPE_PHY;
	a[n].id = SAI_SWITCH_ATTR_HARDWARE_ACCESS_BUS; a[n++].value.s32 = SAI_SWITCH_HARDWARE_ACCESS_BUS_MDIO;
	a[n].id = SAI_SWITCH_ATTR_PLATFROM_CONTEXT;   a[n++].value.u64 = 1;   /* bus 1 = port 1 */
	a[n].id = SAI_SWITCH_ATTR_REGISTER_READ;      a[n++].value.ptr = (void *)reg_read;
	a[n].id = SAI_SWITCH_ATTR_REGISTER_WRITE;     a[n++].value.ptr = (void *)reg_write;
	a[n].id = SAI_SWITCH_ATTR_SWITCH_PROFILE_ID;  a[n++].value.u32 = 0;
	a[n].id = SAI_SWITCH_ATTR_FIRMWARE_LOAD_METHOD; a[n++].value.s32 = SAI_SWITCH_FIRMWARE_LOAD_METHOD_NONE;
	/* FAKE_FW_LOAD_TYPE=skip|force|auto: SAI_SWITCH_ATTR_FIRMWARE_LOAD_TYPE (default: not passed) */
	const char *lt = getenv("FAKE_FW_LOAD_TYPE");
	if (lt) {
		a[n].id = SAI_SWITCH_ATTR_FIRMWARE_LOAD_TYPE;
		a[n++].value.s32 = !strcmp(lt, "skip") ? SAI_SWITCH_FIRMWARE_LOAD_TYPE_SKIP :
				   !strcmp(lt, "force") ? SAI_SWITCH_FIRMWARE_LOAD_TYPE_FORCE :
				   SAI_SWITCH_FIRMWARE_LOAD_TYPE_AUTO;
	}
	/* Deliberately NOT set: SWITCH_HARDWARE_INFO (would use kernel MII ioctls),
	 * SWITCH_SHELL_ENABLE (Lua debug shell), FIRMWARE_PATH_NAME. */

	printf("# phase create_switch\n");
	sai_object_id_t oid = 0;
	sai_status_t st = sw->create_switch(&oid, n, a);
	printf("# create_switch -> %d oid=0x%lx after %lu accesses, %lu us\n",
	       st, (unsigned long)oid, naccess, now_us());
	if (st != SAI_STATUS_SUCCESS)
		return 1;
	if (getenv("FAKE_QUERY"))
		query_objects(query, sw, oid);
	if (getenv("FAKE_WARM_SHUTDOWN")) {
		sai_attribute_t w = { .id = SAI_SWITCH_ATTR_RESTART_WARM, .value.booldata = true };
		printf("# phase warm_shutdown\n");
		printf("# set RESTART_WARM -> %d\n", sw->set_switch_attribute(oid, &w));
		printf("# remove_switch -> %d after %lu accesses\n", sw->remove_switch(oid), naccess);
		sai_status_t (*uninit)(void) = (sai_status_t (*)(void))dlsym(h, "sai_api_uninitialize");
		if (uninit)
			printf("# sai_api_uninitialize -> %d after %lu accesses\n", uninit(), naccess);
	}
	printf("# phase end\n");
	return 0;
}
