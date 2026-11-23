// SPDX-License-Identifier: GPL-2.0
#include <stdlib.h>
#include <string.h>
#include "mock.h"
#include "al_hal_plat_services.h"

FILE *mock_out;
uint8_t *mock_bar_mem[NBARS];
static size_t bar_size[NBARS];
static const char bar_name[NBARS] = { 'u', 'e', 'm', 'p' };

struct pin { int bar; uint32_t off; uint32_t val; };
static struct pin pins[512];
static int npins;

struct ring { char name[8]; uint32_t *mem; uint32_t *shadow; unsigned int entries; };
static struct ring rings[16];
static int nrings;

void mock_init(void)
{
	/* sizes: struct unit_regs, al_ec_regs, al_eth_mac_regs rounded up; PCI config 4 KiB */
	static const size_t sz[NBARS] = { 0x20000, 0x4000, 0x1000, 0x1000 };
	int i;

	mock_out = stdout;
	for (i = 0; i < NBARS; i++) {
		bar_size[i] = sz[i];
		mock_bar_mem[i] = aligned_alloc(64, sz[i]);
		memset(mock_bar_mem[i], 0, sz[i]);
	}
}

uint32_t mock_hash(uint32_t seed, int bar, uint32_t off)
{
	uint32_t h;

	if (seed == 0)
		return 0;
	h = seed ^ ((uint32_t)bar * 0x9E3779B9u) ^ (off * 0x85EBCA6Bu);
	h ^= h >> 16;
	h *= 0x7FEB352Du;
	h ^= h >> 15;
	h *= 0x846CA68Bu;
	h ^= h >> 16;
	return h;
}

void mock_scenario(const char *name)
{
	int i;

	for (i = 0; i < nrings; i++) {
		free(rings[i].shadow);
	}
	nrings = 0;
	npins = 0;
	for (i = 0; i < NBARS; i++)
		memset(mock_bar_mem[i], 0, bar_size[i]);
	fprintf(mock_out, "S %s\n", name);
}

void mock_fill(uint32_t seed)
{
	int b;
	uint32_t off;

	fprintf(mock_out, "I FILL %u\n", seed);
	for (b = 0; b < NBARS; b++)
		for (off = 0; off < bar_size[b]; off += 4)
			*(uint32_t *)(mock_bar_mem[b] + off) = mock_hash(seed, b, off);
}

void mock_pin(int bar, uint32_t off, uint32_t val)
{
	fprintf(mock_out, "I PIN %c %05x %08x\n", bar_name[bar], off, val);
	pins[npins].bar = bar;
	pins[npins].off = off;
	pins[npins].val = val;
	npins++;
}

void mock_set(int bar, uint32_t off, uint32_t val)
{
	fprintf(mock_out, "I SET %c %05x %08x\n", bar_name[bar], off, val);
	*(uint32_t *)(mock_bar_mem[bar] + off) = val;
}

uint32_t mock_peek(int bar, uint32_t off)
{
	return *(uint32_t *)(mock_bar_mem[bar] + off);
}

static int find(const volatile void *p, uint32_t *off)
{
	int b;

	for (b = 0; b < BAR_P; b++) {
		const uint8_t *base = mock_bar_mem[b];
		if ((const uint8_t *)p >= base && (const uint8_t *)p < base + bar_size[b]) {
			*off = (uint32_t)((const uint8_t *)p - base);
			return b;
		}
	}
	fprintf(stderr, "mock: access outside the BARs: %p\n", (const void *)p);
	abort();
}

static uint32_t pinned(int b, uint32_t off, uint32_t v)
{
	int i;

	for (i = 0; i < npins; i++)
		if (pins[i].bar == b && pins[i].off == off)
			return pins[i].val;
	return v;
}

uint32_t mock_r32(const volatile void *p)
{
	uint32_t off;
	int b = find(p, &off);
	uint32_t v = pinned(b, off, *(uint32_t *)(mock_bar_mem[b] + off));

	fprintf(mock_out, "R %c %05x %08x\n", bar_name[b], off, v);
	return v;
}

void mock_w32(volatile void *p, uint32_t v)
{
	uint32_t off;
	int b = find(p, &off);

	*(uint32_t *)(mock_bar_mem[b] + off) = v;
	fprintf(mock_out, "W %c %05x %08x\n", bar_name[b], off, v);
}

uint16_t mock_r16(const volatile void *p)
{
	uint32_t off;
	int b = find(p, &off);
	uint32_t w = pinned(b, off & ~3u, *(uint32_t *)(mock_bar_mem[b] + (off & ~3u)));
	uint16_t v = (uint16_t)(w >> ((off & 2) * 8));

	fprintf(mock_out, "R16 %c %05x %04x\n", bar_name[b], off, v);
	return v;
}

void mock_w16(volatile void *p, uint16_t v)
{
	uint32_t off;
	int b = find(p, &off);

	*(uint16_t *)(mock_bar_mem[b] + off) = v;
	fprintf(mock_out, "W16 %c %05x %04x\n", bar_name[b], off, v);
}

uint8_t mock_r8(const volatile void *p)
{
	fprintf(stderr, "mock: 8-bit read at %p: not modelled\n", (const void *)p);
	abort();
}

void mock_w8(volatile void *p, uint8_t v)
{
	fprintf(stderr, "mock: 8-bit write %02x at %p: not modelled\n", v, (void *)p);
	abort();
}

void mock_udelay(unsigned int us)
{
	fprintf(mock_out, "D %u\n", us);
}

void mock_mb(void)
{
	fprintf(mock_out, "MB\n");
}

int mock_pci_read(void *handle, int where, uint32_t *val)
{
	(void)handle;
	*val = *(uint32_t *)(mock_bar_mem[BAR_P] + where);
	fprintf(mock_out, "CR %03x %08x\n", where, *val);
	return 0;
}

int mock_pci_write(void *handle, int where, uint32_t val)
{
	(void)handle;
	*(uint32_t *)(mock_bar_mem[BAR_P] + where) = val;
	fprintf(mock_out, "CW %03x %08x\n", where, val);
	return 0;
}

int mock_ring_add(const char *name, void *mem, unsigned int entries)
{
	struct ring *r = &rings[nrings];

	snprintf(r->name, sizeof(r->name), "%s", name);
	r->mem = mem;
	r->entries = entries;
	r->shadow = calloc(entries, 16);
	memcpy(r->shadow, mem, entries * 16);
	return nrings++;
}

void mock_rings_clear(void)
{
	int i;

	for (i = 0; i < nrings; i++)
		free(rings[i].shadow);
	nrings = 0;
}

const char *mock_ring_name(int ring)
{
	return rings[ring].name;
}

void mock_ring_dump(void)
{
	int i;
	unsigned int e;

	for (i = 0; i < nrings; i++) {
		struct ring *r = &rings[i];
		for (e = 0; e < r->entries; e++) {
			uint32_t *m = r->mem + e * 4, *s = r->shadow + e * 4;
			if (memcmp(m, s, 16) == 0)
				continue;
			fprintf(mock_out, "DESC %s %u %08x %08x %08x %08x\n", r->name, e,
				m[0], m[1], m[2], m[3]);
			memcpy(s, m, 16);
		}
	}
}

void mock_cdesc(int ring, unsigned int idx, const uint32_t w[4])
{
	struct ring *r = &rings[ring];

	fprintf(mock_out, "I CDESC %s %u %08x %08x %08x %08x\n", r->name, idx,
		w[0], w[1], w[2], w[3]);
	memcpy(r->mem + idx * 4, w, 16);
	memcpy(r->shadow + idx * 4, w, 16);
}

static uint32_t rs = 1;
void rnd_seed(uint32_t s) { rs = s ? s : 1; }
uint32_t rnd(void)
{
	rs ^= rs << 13;
	rs ^= rs >> 17;
	rs ^= rs << 5;
	return rs;
}
