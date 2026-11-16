/* Stand-in for the vendor phy_server libraries, for testing the
 * cs8210-macsec shim + client offline. Same exported names and the same
 * local layout the shim reads (devices, g_slice); behaviour is a small
 * model of the SecY/CfyE calls, logged to stderr. Not vendor code. */
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { uint32_t lo, hi; } cnt_t;
uint8_t devices[8 + 16 * 0x1b8];
void *g_slice;
static int slices[16];
static bool dev_init[32];
static int nsa, nrule, nvp;
struct fsa { int used, dev, vport, type; uint32_t seq; };
static struct fsa sas[256];
static void *active_tx[32][64];

#define LOG(...) fprintf(stderr, "fake: " __VA_ARGS__)

static int dp_init(char *l)
{
	int s = atoi(strstr(l, "slice_id=") + 9);
	dev_init[2 * (s - 1)] = dev_init[2 * (s - 1) + 1] = true;
	LOG("macsec_datapath_init slice %d\n", s);
	return 0;
}
struct { char name[512]; int (*fn)(char *); } credo_cmds[0x41] = {
	{ "macsec_datapath_init slice_id=(1|0x2|all)", dp_init },
};

__attribute__((constructor)) static void setup(void)
{
	for (int k = 0; k < 16; k++) {
		*(uint32_t *)(devices + 8 + k * 0x1b8) = k + 1;
		*(void **)(devices + 8 + k * 0x1b8 + 0x10) = &slices[k];
	}
}
static void chk_slice(unsigned dev)
{
	if (g_slice != &slices[dev / 2])
		LOG("BUG: g_slice not set for dev %u\n", dev);
}

int SecY_Device_Role_Get(unsigned d, int *r) { if (!dev_init[d]) return 2; *r = d & 1 ? 0 : 1; return 0; }
int SecY_Device_Limits(unsigned d, unsigned *c, unsigned *v, unsigned *s, unsigned *sc)
{ if (!dev_init[d]) return 2; *c = 8; *v = 64; *s = 128; *sc = 64; return 0; }
int CfyE_Device_Limits(unsigned d, unsigned *c, unsigned *v, unsigned *r)
{ if (!dev_init[d]) return 2; *c = 8; *v = 64; *r = 128; return 0; }
int CfyE_vPort_Add(unsigned d, void **h, void *vp, unsigned u)
{ (void)vp; (void)u; chk_slice(d); *h = (void *)(uintptr_t)(0x1000 + ++nvp); LOG("vPort_Add dev %u -> %d\n", d, nvp); return 0; }
int CfyE_vPort_Remove(unsigned d, void *h) { LOG("vPort_Remove dev %u %p\n", d, h); return 0; }
int CfyE_vPortIndex_Get(void *h, unsigned *i) { *i = (uintptr_t)h - 0x1001; return 0; }
typedef struct { uint32_t pt; uint8_t nt; uint16_t ch; uint32_t mpt; uint8_t mnt; uint16_t mch;
	uint32_t data[4], mask[4]; void *vp; uint8_t prio; bool drop, ctl; } rule_t;
int CfyE_Rule_Add(unsigned d, void *vp, void **h, rule_t *r)
{ chk_slice(d); *h = (void *)(uintptr_t)(0x2000 + ++nrule);
  LOG("Rule_Add dev %u vp %p data2 %#x prio %u drop %d ctl %d\n", d, vp, r->data[2], r->prio, r->drop, r->ctl); return 0; }
int CfyE_Rule_Update(unsigned d, void *h, rule_t *r) { LOG("Rule_Update dev %u %p drop %d\n", d, h, r->drop); return 0; }
int CfyE_Rule_Enable(unsigned d, void *h, bool s) { (void)d; (void)h; (void)s; return 0; }
int CfyE_Rule_Remove(unsigned d, void *h) { LOG("Rule_Remove dev %u %p\n", d, h); return 0; }
int CfyE_RuleIndex_Get(void *h, unsigned *i) { *i = (uintptr_t)h - 0x2001; return 0; }
int CfyE_Statistics_TCAM_Get(unsigned d, unsigned i, cnt_t *c, bool s) { (void)d; (void)s; c->lo = 100 + i; c->hi = 0; return 0; }

typedef struct { unsigned wc; uint32_t *tr; uint8_t p[0x18]; uint32_t act, drop, port; } sa_t;
static int sa_new(unsigned d, unsigned vp, sa_t *s, void **h)
{
	chk_slice(d);
	if (s->act >= 2 && (!s->tr || !s->wc)) { LOG("BUG: no transform record\n"); return 2; }
	if (s->act >= 2 && (s->tr[0] & 0x400)) LOG("BUG: ROLLOVER set\n");
	int i = ++nsa;
	sas[i] = (struct fsa){ 1, d, vp, s->act, s->act >= 2 ? s->tr[s->act == 3 ? 14 : 14] : 0 };
	*h = &sas[i];
	LOG("SA_Add dev %u vport %u act %u words %u ctrl %#x\n", d, vp, s->act, s->wc, s->tr ? s->tr[0] : 0);
	return 0;
}
int SecY_SA_Add(unsigned d, unsigned vp, void **h, sa_t *s)
{
	int r = sa_new(d, vp, s, h);
	if (!r && s->act == 3) {
		if (active_tx[d][vp]) LOG("BUG: SA_Add over an active egress SA\n");
		active_tx[d][vp] = *h;
	}
	return r;
}
int SecY_SA_Chain(unsigned d, void *act, void **h, sa_t *s)
{
	struct fsa *a = act;
	if (!a || active_tx[d][a->vport] != act) { LOG("BUG: chain on non-active SA\n"); return 2; }
	LOG("SA_Chain\n");
	return sa_new(d, a->vport, s, h);
}
int SecY_SA_Switch(unsigned d, void *act, void *n)
{
	struct fsa *a = act;
	if (active_tx[d][a->vport] != act) { LOG("BUG: switch from non-active\n"); return 2; }
	active_tx[d][a->vport] = n;
	LOG("SA_Switch\n");
	return 0;
}
int SecY_SA_Remove(unsigned d, void *h)
{
	struct fsa *a = h;
	if (a->type == 3 && active_tx[d][a->vport] == h) active_tx[d][a->vport] = NULL;
	LOG("SA_Remove dev %u type %d\n", d, a->type);
	a->used = 0;
	return 0;
}
int SecY_SA_Update(unsigned d, void *h, sa_t *s) { (void)h; LOG("SA_Update dev %u act %u inuse %u\n", d, s->act, s->p[0]); return 0; }
int SecY_SA_NextPN_Get(unsigned d, void *h, uint32_t *lo, uint32_t *hi, bool *x) { (void)d; *lo = ((struct fsa *)h)->seq; *hi = 0; *x = 0; return 0; }
int SecY_SA_NextPN_Update(unsigned d, void *h, uint32_t lo, uint32_t hi, bool *w) { (void)d; (void)hi; ((struct fsa *)h)->seq = lo; *w = 1; return 0; }
int SecY_SA_WindowSize_Update(unsigned d, void *h, uint32_t w) { (void)d; (void)h; (void)w; return 0; }
int SecY_SA_Active_E_Get(unsigned d, unsigned v, void **h) { *h = active_tx[d][v]; return 0; }
int SecY_SA_Statistics_E_Get(unsigned d, void *h, cnt_t *c, bool s) { (void)d; (void)h; (void)s; for (int i = 0; i < 4; i++) c[i] = (cnt_t){ i + 1, 1 }; return 0; }
int SecY_SA_Statistics_I_Get(unsigned d, void *h, cnt_t *c, bool s) { (void)d; (void)h; (void)s; for (int i = 0; i < 10; i++) c[i] = (cnt_t){ i + 1, 0 }; return 0; }
int SecY_SecY_Statistics_E_Get(unsigned d, unsigned v, cnt_t *c, bool s) { (void)d; (void)v; (void)s; for (int i = 0; i < 3; i++) c[i] = (cnt_t){ 7, 0 }; return 0; }
int SecY_SecY_Statistics_I_Get(unsigned d, unsigned v, cnt_t *c, bool s) { (void)d; (void)v; (void)s; for (int i = 0; i < 8; i++) c[i] = (cnt_t){ 8, 0 }; return 0; }
int SecY_Ifc_Statistics_E_Get(unsigned d, unsigned v, cnt_t *c, bool s) { (void)d; (void)v; (void)s; for (int i = 0; i < 9; i++) c[i] = (cnt_t){ 9, 0 }; return 0; }
int SecY_Ifc_Statistics_I_Get(unsigned d, unsigned v, cnt_t *c, bool s) { (void)d; (void)v; (void)s; for (int i = 0; i < 8; i++) c[i] = (cnt_t){ 9, 0 }; return 0; }
int SecY_SAIndex_Get(void *h, unsigned *s, unsigned *c) { *s = (struct fsa *)h - sas; *c = 0; return 0; }
typedef struct { uint32_t flags, dir, op; uint8_t an; uint8_t *key; unsigned klen; uint8_t *hkey, *salt, *ssci, *sci;
	uint32_t lo, hi, win; unsigned icv; } sab_t;
int SABuilder_InitParams(sab_t *p, uint8_t an, uint32_t dir, uint32_t op) { memset(p, 0, 0x50); p->an = an; p->dir = dir; p->op = op; if (dir == 1) p->lo = 1; return 0; }
int SABuilder_GetSize(sab_t *p, unsigned *w) { *w = p->dir ? 20 : 22; return 0; }
int SABuilder_BuildSA(sab_t *p, uint32_t *b, void *cb)
{
	(void)cb;
	if (!p->key || !p->hkey || (p->klen != 16 && p->klen != 32)) return 1;
	if ((p->flags & 1) && (!p->salt || !p->ssci)) return 1;
	b[0] = (p->flags & 1 ? 0x20000000 : 0x10000000) | (p->flags & 8 ? 0x400 : 0) | p->an << 26;
	b[14] = p->lo;
	return 0;
}
int get_channel_id(void *s, int port, unsigned *ch) { (void)s; *ch = port; return 0; }
int enable_low_latency_bypass(void *s, int port, int side, int byp) { (void)s; LOG("low_latency_bypass port %d side %d bypass %d\n", port, side, byp); return 0; }
