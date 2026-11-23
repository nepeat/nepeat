// SPDX-License-Identifier: GPL-2.0
/*
 * Recorder of al_eth_trace (see al_eth_trace.h): a fixed array of records,
 * claimed with an atomic counter (any CPU, any context), read back as text
 * from /proc/al_eth_trace in the difftest trace format of
 * ../difftest/mock.h (bar u = BAR0 UDMA, m = BAR2 MAC, e = BAR4 EC, p = PCI
 * config), each line ending in " [pid]" (process context: the task) or " @"
 * (softirq/hardirq context: NAPI, xmit), so concurrent callers separate.
 * Lost when the module is unloaded: copy the file first.
 */
#include <linux/atomic.h>
#include <linux/delay.h>
#include <linux/io.h>
#include <linux/module.h>
#include <linux/overflow.h>
#include <linux/pci.h>
#include <linux/preempt.h>
#include <linux/sched.h>
#include <linux/proc_fs.h>
#include <linux/seq_file.h>
#include <linux/smp.h>
#include <linux/vmalloc.h>
#include "al_eth_trace.h"

static unsigned int trace_records = 1u << 21;
module_param(trace_records, uint, 0444);
MODULE_PARM_DESC(trace_records, "records kept (16 bytes each), later accesses are counted as dropped");

struct al_trace_rec {
	u8 op;
	u8 bar;
	u8 ctx;
	u8 cpu;
	u32 pid;
	u32 off;
	u32 val;
};

static struct al_trace_rec *recs;
static atomic_t nrecs = ATOMIC_INIT(0);
static atomic_t ndropped = ATOMIC_INIT(0);
static void __iomem *bar_base[3];
static resource_size_t bar_len[3];
static const char bar_name[3] = { 'u', 'm', 'e' };
static const int bar_index[3] = { 0, 2, 4 };
static const char *const mark_name[AL_TRACE_NMARKS] = {
	"probe", "open", "close", "remove", "rx_mode", "mtu", "set_mac", "stats", "bars",
};

static void put_raw(u8 op, u8 bar, u32 off, u32 v)
{
	struct al_trace_rec *r;
	unsigned int i;

	if (!recs)
		return;
	i = atomic_inc_return(&nrecs) - 1;
	if (i >= trace_records) {
		atomic_inc(&ndropped);
		return;
	}
	r = &recs[i];
	r->op = op;
	r->bar = bar;
	r->ctx = in_interrupt() ? 1 : 0;
	r->cpu = raw_smp_processor_id();
	r->pid = in_interrupt() ? 0 : task_pid_nr(current);
	r->off = off;
	r->val = v;
}

static void put(u8 op, const volatile void __iomem *p, u32 v)
{
	u8 bar = '?';
	u32 off = (u32)(unsigned long)p;
	int b;

	if (p) {
		for (b = 0; b < 3; b++) {
			const volatile u8 __iomem *s = bar_base[b];
			if (s && (const volatile u8 __iomem *)p >= s &&
			    (const volatile u8 __iomem *)p < s + bar_len[b]) {
				bar = bar_name[b];
				off = (u32)((const volatile u8 __iomem *)p - s);
				break;
			}
		}
	}
	put_raw(op, bar, off, v);
}

u32 al_trace_r32(const volatile void __iomem *p)
{
	u32 v = readl(p);

	put('R', p, v);
	return v;
}

void al_trace_w32(volatile void __iomem *p, u32 v)
{
	put('W', p, v);
	writel(v, p);
}

u32 al_trace_r32_relaxed(const volatile void __iomem *p)
{
	u32 v = readl_relaxed(p);

	put('R', p, v);
	return v;
}

void al_trace_w32_relaxed(volatile void __iomem *p, u32 v)
{
	put('W', p, v);
	writel_relaxed(v, p);
}

u16 al_trace_r16(const volatile void __iomem *p)
{
	u16 v = readw(p);

	put('r', p, v);
	return v;
}

void al_trace_w16(volatile void __iomem *p, u16 v)
{
	put('w', p, v);
	writew(v, p);
}

u8 al_trace_r8(const volatile void __iomem *p)
{
	u8 v = readb(p);

	put('b', p, v);
	return v;
}

void al_trace_w8(volatile void __iomem *p, u8 v)
{
	put('B', p, v);
	writeb(v, p);
}

void al_trace_udelay(unsigned long us)
{
	put('D', NULL, us);
	udelay(us);
}

void al_trace_cfg(int write, int where, u32 v)
{
	put_raw(write ? 'c' : 'C', 'p', where, v);
}

void al_trace_mark(enum al_trace_mark m)
{
	put('K', NULL, m);
}

void al_trace_bars(struct pci_dev *pdev)
{
	void __iomem * const *t = pcim_iomap_table(pdev);
	int b;

	if (!t)
		return;
	for (b = 0; b < 3; b++) {
		bar_base[b] = t[bar_index[b]];
		bar_len[b] = pci_resource_len(pdev, bar_index[b]);
	}
	al_trace_mark(AL_TRACE_BARS);
}

static void *tr_start(struct seq_file *s, loff_t *pos)
{
	unsigned int n = min_t(unsigned int, atomic_read(&nrecs), trace_records);

	if (*pos == 0)
		return SEQ_START_TOKEN;
	return (*pos <= n) ? &recs[*pos - 1] : NULL;
}

static void *tr_next(struct seq_file *s, void *v, loff_t *pos)
{
	++*pos;
	return tr_start(s, pos);
}

static void tr_stop(struct seq_file *s, void *v)
{
}

static int tr_show(struct seq_file *s, void *v)
{
	const struct al_trace_rec *r = v;
	char ctx[16];

	if (v == SEQ_START_TOKEN) {
		seq_printf(s, "# al_eth_trace: %u records, %u dropped; bar sizes u %pa m %pa e %pa\n",
			   min_t(unsigned int, atomic_read(&nrecs), trace_records), atomic_read(&ndropped),
			   &bar_len[0], &bar_len[1], &bar_len[2]);
		return 0;
	}
	if (r->ctx)
		strscpy(ctx, " @", sizeof(ctx));
	else
		snprintf(ctx, sizeof(ctx), " [%u]", r->pid);
	switch (r->op) {
	case 'R':
	case 'W':
		seq_printf(s, "%c %c %05x %08x%s\n", r->op, r->bar, r->off, r->val, ctx);
		break;
	case 'r':
	case 'w':
		seq_printf(s, "%c16 %c %05x %04x%s\n", r->op == 'r' ? 'R' : 'W', r->bar, r->off, r->val, ctx);
		break;
	case 'b':
	case 'B':
		seq_printf(s, "%c8 %c %05x %02x%s\n", r->op == 'b' ? 'R' : 'W', r->bar, r->off, r->val, ctx);
		break;
	case 'D':
		seq_printf(s, "D %u%s\n", r->val, ctx);
		break;
	case 'C':
	case 'c':
		seq_printf(s, "C%c %03x %08x%s\n", r->op == 'C' ? 'R' : 'W', r->off, r->val, ctx);
		break;
	case 'K':
		seq_printf(s, "# mark %s cpu %u%s\n", r->val < AL_TRACE_NMARKS ? mark_name[r->val] : "?", r->cpu, ctx);
		break;
	}
	return 0;
}

static const struct seq_operations tr_seq_ops = {
	.start = tr_start,
	.next = tr_next,
	.stop = tr_stop,
	.show = tr_show,
};

static int tr_open(struct inode *inode, struct file *file)
{
	return seq_open(file, &tr_seq_ops);
}

static const struct proc_ops tr_proc_ops = {
	.proc_open = tr_open,
	.proc_read = seq_read,
	.proc_lseek = seq_lseek,
	.proc_release = seq_release,
};

int al_trace_init(void)
{
	recs = vzalloc(array_size(trace_records, sizeof(*recs)));
	if (!recs)
		return -ENOMEM;
	if (!proc_create("al_eth_trace", 0400, NULL, &tr_proc_ops)) {
		vfree(recs);
		recs = NULL;
		return -ENOMEM;
	}
	return 0;
}

void al_trace_exit(void)
{
	remove_proc_entry("al_eth_trace", NULL);
	vfree(recs);
	recs = NULL;
}
