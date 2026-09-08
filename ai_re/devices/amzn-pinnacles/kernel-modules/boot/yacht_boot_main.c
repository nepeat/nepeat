// SPDX-License-Identifier: GPL-2.0
/* RAM-only recovery warm boot for Amazon yacht (MT8183, Amazon 4.4.146). */
#include <linux/err.h>
#include <linux/atomic.h>
#include <linux/fs.h>
#include <linux/gfp.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/smp.h>
#include <linux/types.h>
#include <linux/uaccess.h>
#include <linux/vmalloc.h>
#include <asm/io.h>
#include <asm/memory.h>
#include <asm/pgtable-hwdef.h>

#ifndef YACHT_JUMP_MODE
#define YACHT_JUMP_MODE 0
#endif

#define YACHT_IMAGE_PA       0x40080000ULL
#define YACHT_DTB_PA         0x54000000ULL
#define YACHT_RAMDISK_PA     0x55000000ULL
#define YACHT_WDT_PA         0x10007000ULL
#define YACHT_WDT_MODE       0x00
#define YACHT_WDT_LENGTH     0x04
#define YACHT_WDT_RST        0x08
#define YACHT_WDT_LENGTH_KEY 0x00000008U
#define YACHT_WDT_RST_RELOAD 0x1971U
#define YACHT_WDT_NONRST2    0x24
#define YACHT_WDT_KEY        0x22000000U
#define YACHT_WDT_ENABLE     0x00000001U
#define YACHT_WDT_EXT_POL_HIGH 0x00000002U
#define YACHT_WDT_EXRST_ENABLE 0x00000004U
#define YACHT_WDT_IRQ_ENABLE 0x00000008U
#define YACHT_WDT_AUTO_START 0x00000010U
#define YACHT_WDT_IRQ_LEVEL  0x00000020U
#define YACHT_WDT_DUAL_ENABLE 0x00000040U
#define YACHT_WDT_TIMEOUT_SECONDS 30U
#define YACHT_MARKER         0x545448435941cafeULL
#define YACHT_PHASE_BASE     0x5941434800000000ULL
#define YACHT_MARKER_OFFSET  0xff0
#define YACHT_MAX_LIST_PAGES 64
#define YACHT_MAX_QUARANTINE 16384
#define YACHT_REHEARSAL_CHUNK_PAGES 4
#define YACHT_SECONDARY_CPUS 7
#define YACHT_LINEAR_BASE    0xffffffc000000000ULL
#define YACHT_PHYS_BASE      0x40000000ULL

extern unsigned long kallsyms_lookup_name(const char *name);
extern char yacht_control_start[], yacht_control_end[];
extern char yacht_rehearse_entry[], yacht_copy_test_entry[];
extern char yacht_list_copy_test_entry[], yacht_jump_entry[];
extern void yacht_run_identity_test(phys_addr_t pgd, phys_addr_t entry,
				    phys_addr_t control, phys_addr_t low_entry);
extern void yacht_run_copy_test(phys_addr_t pgd, phys_addr_t low_entry,
				phys_addr_t source, phys_addr_t destination);
extern void yacht_run_list_copy_test(phys_addr_t pgd, phys_addr_t low_entry,
				     phys_addr_t list, bool invalidate_low);
extern void yacht_enter_jump(phys_addr_t pgd, phys_addr_t jump_entry,
			     phys_addr_t list, phys_addr_t image,
			     phys_addr_t dtb) __noreturn;
extern long yacht_psci_cpu_off(void);
struct yacht_payload {
	const char *name;
	const char *path;
	size_t size;
	phys_addr_t destination;
	void **pages;
	unsigned int page_count;
};

struct yacht_pair {
	u64 source;
	u64 destination;
};

struct yacht_list_page {
	u64 next;
	u64 count;
	struct yacht_pair pair[255];
};

static struct yacht_payload payloads[] = {
	{ "Image", "/mnt/yacht-stage/Image", 25932040, YACHT_IMAGE_PA, NULL },
	{ "ramdisk-adb-smoke.gz", "/mnt/yacht-stage/ramdisk-adb-smoke.gz", 15344410,
	  YACHT_RAMDISK_PA, NULL },
	{ "yacht-live-recovery-adb-headless.dtb",
	  "/mnt/yacht-stage/yacht-live-recovery-adb-headless.dtb",
	  215367,
	  YACHT_DTB_PA, NULL },
};

static void *(*yacht_vzalloc)(unsigned long);
static void (*yacht_vfree)(const void *);
static struct file *(*yacht_filp_open)(const char *, int, umode_t);
static int (*yacht_filp_close)(struct file *, fl_owner_t);
static ssize_t (*yacht_vfs_read)(struct file *, char __user *, size_t, loff_t *);
static unsigned long (*yacht_get_free_pages)(gfp_t, unsigned int);
static void (*yacht_free_pages)(unsigned long, unsigned int);
static void (*yacht_flush_dcache_area)(void *, size_t);
static void __iomem *(*yacht_ioremap)(phys_addr_t, size_t, pgprot_t);
static void (*yacht_iounmap)(volatile void __iomem *);
static void (*yacht_kernel_restart_prepare)(char *);
static void (*yacht_migrate_to_reboot_cpu)(void);
static void (*yacht_aee_rr_rec_last_init_func)(unsigned long);
static int (*yacht_smp_call_function)(smp_call_func_t, void *, int);
static void (*yacht_emergency_restart)(void) __noreturn;
static atomic_t yacht_secondary_arrived = ATOMIC_INIT(0);
static atomic_t yacht_secondary_parked = ATOMIC_INIT(0);

static struct yacht_list_page *list_virt[YACHT_MAX_LIST_PAGES];
static unsigned int list_pages;
static struct yacht_list_page *scratch_list_virt[YACHT_MAX_LIST_PAGES];
static unsigned int scratch_list_pages;
static struct yacht_list_page *scratch_chunk;
static void **scratch_pages;
static unsigned int scratch_page_count;
static void *control_page;
static u64 *identity_pgd;
static u64 *copy_source;
static u64 *copy_destination;
static phys_addr_t first_list_pa;
static void *quarantine[YACHT_MAX_QUARANTINE];
static unsigned int quarantine_count;

/* Exact yacht VA_BITS=39 linear-map conversion, avoiding private data symbols. */
static phys_addr_t yacht_linear_to_phys(const void *address)
{
	return ((u64)address & ~YACHT_LINEAR_BASE) + YACHT_PHYS_BASE;
}

static void yacht_invalidate_dcache_page(void *page)
{
	u64 ctr;
	unsigned long address = (unsigned long)page;
	unsigned long end = address + PAGE_SIZE;
	unsigned long line_size;

	asm volatile("mrs %0, ctr_el0" : "=r" (ctr));
	line_size = 4UL << ((ctr >> 16) & 0xf);
	address &= ~(line_size - 1);
	for (; address < end; address += line_size)
		asm volatile("dc civac, %0" :: "r" (address) : "memory");
	dsb(sy);
}

static int yacht_resolve(void)
{
#define RESOLVE(field, symbol) do { \
	yacht_##field = (void *)kallsyms_lookup_name(symbol); \
	if (!yacht_##field) { pr_err("yacht_boot: missing %s\n", symbol); return -ENOENT; } \
} while (0)
	RESOLVE(vzalloc, "vzalloc");
	RESOLVE(vfree, "vfree");
	RESOLVE(filp_open, "filp_open");
	RESOLVE(filp_close, "filp_close");
	RESOLVE(vfs_read, "vfs_read");
	RESOLVE(get_free_pages, "__get_free_pages");
	RESOLVE(free_pages, "free_pages");
	RESOLVE(flush_dcache_area, "__flush_dcache_area");
	RESOLVE(ioremap, "__ioremap");
	RESOLVE(iounmap, "__iounmap");
	RESOLVE(kernel_restart_prepare, "kernel_restart_prepare");
	RESOLVE(migrate_to_reboot_cpu, "migrate_to_reboot_cpu");
	RESOLVE(aee_rr_rec_last_init_func, "aee_rr_rec_last_init_func");
	RESOLVE(smp_call_function, "smp_call_function");
	RESOLVE(emergency_restart, "emergency_restart");
#undef RESOLVE
	return 0;
}

static u64 yacht_counter(void)
{
	u64 value;

	asm volatile("mrs %0, cntpct_el0" : "=r" (value));
	return value;
}

static u64 yacht_counter_frequency(void)
{
	u64 value;

	asm volatile("mrs %0, cntfrq_el0" : "=r" (value));
	return value;
}

static void yacht_wait_ms(unsigned int milliseconds)
{
	u64 start = yacht_counter();
	u64 ticks = yacht_counter_frequency() * milliseconds / 1000;

	while (yacht_counter() - start < ticks)
		cpu_relax();
}

/*
 * Five yacht secondaries accept PSCI CPU_OFF.  Two return an error instead.
 * Any returner masks every exception class and parks forever, so it cannot
 * race the MMU-off relocation or the single-core recovery kernel.
 */
static void yacht_off_or_park_secondary(void *unused)
{
	unsigned int attempt;

	atomic_inc(&yacht_secondary_arrived);
	smp_mb();
	for (attempt = 0; attempt < 3; attempt++)
		yacht_psci_cpu_off();
	atomic_inc(&yacht_secondary_parked);
	asm volatile("msr daifset, #0xf; dsb sy; isb" ::: "memory");
	for (;;)
		asm volatile("wfe" ::: "memory");
}

static int yacht_quiesce_secondaries(void)
{
	unsigned int waited;
	int error;

	atomic_set(&yacht_secondary_arrived, 0);
	atomic_set(&yacht_secondary_parked, 0);
	smp_wmb();
	error = yacht_smp_call_function(yacht_off_or_park_secondary, NULL, 0);
	if (error)
		return error;
	for (waited = 0; waited < 2000; waited++) {
		if (atomic_read(&yacht_secondary_arrived) == YACHT_SECONDARY_CPUS)
			break;
		yacht_wait_ms(1);
	}
	if (atomic_read(&yacht_secondary_arrived) != YACHT_SECONDARY_CPUS)
		return -ETIMEDOUT;

	/* Allow all returning PSCI calls to reach their permanent park loop. */
	yacht_wait_ms(500);
	pr_emerg("yacht_boot: secondary fanout arrived=%d parked=%d inferred_off=%d\n",
		 atomic_read(&yacht_secondary_arrived),
		 atomic_read(&yacht_secondary_parked),
		 YACHT_SECONDARY_CPUS - atomic_read(&yacht_secondary_parked));
	return 0;
}

static bool yacht_overlaps_destination(phys_addr_t page)
{
	unsigned int i;

	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		phys_addr_t start = payloads[i].destination;
		phys_addr_t end = start + PAGE_ALIGN(payloads[i].size);
		if (page < end && page + PAGE_SIZE > start)
			return true;
	}
	return false;
}

static void *yacht_alloc_safe_page(void)
{
	while (quarantine_count < YACHT_MAX_QUARANTINE) {
		unsigned long address = yacht_get_free_pages(GFP_KERNEL | __GFP_ZERO, 0);
		if (!address)
			return NULL;
		if (!yacht_overlaps_destination(yacht_linear_to_phys((void *)address)))
			return (void *)address;
		/* Hold excluded pages until every source/control page is allocated. */
		quarantine[quarantine_count++] = (void *)address;
	}
	return NULL;
}

static void yacht_release_quarantine(void)
{
	while (quarantine_count)
		yacht_free_pages((unsigned long)quarantine[--quarantine_count], 0);
}

static int yacht_read_file(struct yacht_payload *payload)
{
	struct file *file;
	mm_segment_t old_fs;
	loff_t position = 0;
	size_t done = 0;
	unsigned int i;

	file = yacht_filp_open(payload->path, O_RDONLY, 0);
	if (IS_ERR(file))
		return PTR_ERR(file);
	payload->page_count = PAGE_ALIGN(payload->size) / PAGE_SIZE;
	payload->pages = yacht_vzalloc(payload->page_count * sizeof(*payload->pages));
	if (!payload->pages) {
		yacht_filp_close(file, NULL);
		return -ENOMEM;
	}
	for (i = 0; i < payload->page_count; i++) {
		payload->pages[i] = yacht_alloc_safe_page();
		if (!payload->pages[i]) {
			yacht_filp_close(file, NULL);
			return -ENOMEM;
		}
	}
	old_fs = get_fs();
	set_fs(KERNEL_DS);
	while (done < payload->size) {
		size_t page_offset = done & (PAGE_SIZE - 1);
		size_t wanted = min_t(size_t, PAGE_SIZE - page_offset,
				      payload->size - done);
		ssize_t count = yacht_vfs_read(file,
			payload->pages[done / PAGE_SIZE] + page_offset,
			wanted, &position);
		if (count <= 0)
			break;
		done += count;
	}
	set_fs(old_fs);
	yacht_filp_close(file, NULL);
	return done == payload->size ? 0 : -EIO;
}

static int yacht_validate_content(void)
{
	const u8 *image = payloads[0].pages[0];
	const u8 *ramdisk = payloads[1].pages[0];
	const u8 *dtb = payloads[2].pages[0];
	u32 fdt_size;

	if (image[0x38] != 'A' || image[0x39] != 'R' ||
	    image[0x3a] != 'M' || image[0x3b] != 0x64)
		return -ENOEXEC;
	if (ramdisk[0] != 0x1f || ramdisk[1] != 0x8b)
		return -ENOEXEC;
	if (dtb[0] != 0xd0 || dtb[1] != 0x0d ||
	    dtb[2] != 0xfe || dtb[3] != 0xed)
		return -ENOEXEC;
	fdt_size = ((u32)dtb[4] << 24) | ((u32)dtb[5] << 16) |
		   ((u32)dtb[6] << 8) | dtb[7];
	return fdt_size >= 40 && fdt_size <= payloads[2].size ? 0 : -EINVAL;
}

static int yacht_add_pair(phys_addr_t source, phys_addr_t destination)
{
	struct yacht_list_page *page;

	if (!list_pages || list_virt[list_pages - 1]->count == 255) {
		phys_addr_t pa;
		if (list_pages == YACHT_MAX_LIST_PAGES)
			return -E2BIG;
		page = yacht_alloc_safe_page();
		if (!page)
			return -ENOMEM;
		pa = yacht_linear_to_phys(page);
		if (list_pages)
			list_virt[list_pages - 1]->next = pa;
		else
			first_list_pa = pa;
		list_virt[list_pages++] = page;
	}
	page = list_virt[list_pages - 1];
	page->pair[page->count].source = source;
	page->pair[page->count].destination = destination;
	page->count++;
	return 0;
}

static int yacht_build_copy_list(void)
{
	unsigned int i;

	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		unsigned int page_index;
		for (page_index = 0; page_index < payloads[i].page_count;
		     page_index++) {
			size_t offset = (size_t)page_index * PAGE_SIZE;
			phys_addr_t source =
				yacht_linear_to_phys(payloads[i].pages[page_index]);
			if (yacht_overlaps_destination(source)) {
				pr_err("yacht_boot: %s source %pa overlaps destination\n",
				       payloads[i].name, &source);
				return -EADDRINUSE;
			}
			if (yacht_add_pair(source, payloads[i].destination + offset))
				return -ENOMEM;
		}
	}
	return 0;
}

static int yacht_add_scratch_pair(phys_addr_t source, phys_addr_t destination)
{
	struct yacht_list_page *page;

	if (!scratch_list_pages ||
	    scratch_list_virt[scratch_list_pages - 1]->count == 255) {
		phys_addr_t pa;
		if (scratch_list_pages == YACHT_MAX_LIST_PAGES)
			return -E2BIG;
		page = yacht_alloc_safe_page();
		if (!page)
			return -ENOMEM;
		pa = yacht_linear_to_phys(page);
		if (scratch_list_pages)
			scratch_list_virt[scratch_list_pages - 1]->next = pa;
		scratch_list_virt[scratch_list_pages++] = page;
	}
	page = scratch_list_virt[scratch_list_pages - 1];
	page->pair[page->count].source = source;
	page->pair[page->count].destination = destination;
	page->count++;
	return 0;
}

static int yacht_build_scratch_list(void)
{
	unsigned int i;
	unsigned int total_pages = 0;

	for (i = 0; i < ARRAY_SIZE(payloads); i++)
		total_pages += PAGE_ALIGN(payloads[i].size) / PAGE_SIZE;
	scratch_pages = yacht_vzalloc(total_pages * sizeof(*scratch_pages));
	if (!scratch_pages)
		return -ENOMEM;

	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		unsigned int page_index;
		for (page_index = 0; page_index < payloads[i].page_count;
		     page_index++) {
			phys_addr_t source =
				yacht_linear_to_phys(payloads[i].pages[page_index]);
			void *destination = yacht_alloc_safe_page();
			int error;
			if (!destination)
				return -ENOMEM;
			scratch_pages[scratch_page_count++] = destination;
			error = yacht_add_scratch_pair(source,
					yacht_linear_to_phys(destination));
			if (error)
				return error;
		}
	}
	return 0;
}

static int yacht_prepare_control(void)
{
	size_t length = yacht_control_end - yacht_control_start;
	size_t i;
	u8 *destination;

	if (!length || length > YACHT_MARKER_OFFSET)
		return -E2BIG;
	control_page = yacht_alloc_safe_page();
	identity_pgd = yacht_alloc_safe_page();
	copy_source = yacht_alloc_safe_page();
	copy_destination = yacht_alloc_safe_page();
	scratch_chunk = yacht_alloc_safe_page();
	if (!control_page || !identity_pgd || !copy_source || !copy_destination ||
	    !scratch_chunk)
		return -ENOMEM;
	destination = control_page;
	for (i = 0; i < length; i++)
		destination[i] = yacht_control_start[i];

	/* 39-bit, 4 KiB granule: executable normal-memory 1 GiB blocks. */
	for (i = 1; i <= 4; i++) {
		u64 base = (u64)i << 30;
		identity_pgd[i] = base | 1ULL | (1ULL << 10) | (3ULL << 8) |
				  ((u64)MT_NORMAL << 2);
	}
	return 0;
}

static void yacht_flush_everything(void)
{
	unsigned int i;

	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		unsigned int page_index;
		for (page_index = 0; page_index < payloads[i].page_count;
		     page_index++)
			yacht_flush_dcache_area(payloads[i].pages[page_index], PAGE_SIZE);
	}
	for (i = 0; i < list_pages; i++)
		yacht_flush_dcache_area(list_virt[i], PAGE_SIZE);
	for (i = 0; i < scratch_list_pages; i++)
		yacht_flush_dcache_area(scratch_list_virt[i], PAGE_SIZE);
	yacht_flush_dcache_area(identity_pgd, PAGE_SIZE);
	yacht_flush_dcache_area(control_page, PAGE_SIZE);
	yacht_flush_dcache_area(copy_source, PAGE_SIZE);
	yacht_flush_dcache_area(copy_destination, PAGE_SIZE);
	yacht_flush_dcache_area(scratch_chunk, PAGE_SIZE);
	asm volatile("ic iallu; dsb sy; isb" ::: "memory");
}

static void yacht_free_scratch(void)
{
	unsigned int i;

	for (i = 0; i < scratch_page_count; i++)
		yacht_free_pages((unsigned long)scratch_pages[i], 0);
	scratch_page_count = 0;
	if (scratch_pages) {
		yacht_vfree(scratch_pages);
		scratch_pages = NULL;
	}
	for (i = 0; i < scratch_list_pages; i++)
		yacht_free_pages((unsigned long)scratch_list_virt[i], 0);
	scratch_list_pages = 0;
}

static void yacht_cleanup(void)
{
	unsigned int i;

	yacht_release_quarantine();
	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		unsigned int page_index;
		for (page_index = 0; page_index < payloads[i].page_count;
		     page_index++) {
			if (payloads[i].pages[page_index])
				yacht_free_pages((unsigned long)payloads[i].pages[page_index], 0);
		}
		payloads[i].page_count = 0;
		if (payloads[i].pages && yacht_vfree) {
			yacht_vfree(payloads[i].pages);
			payloads[i].pages = NULL;
		}
	}
	for (i = 0; i < list_pages; i++)
		yacht_free_pages((unsigned long)list_virt[i], 0);
	list_pages = 0;
	yacht_free_scratch();
	if (identity_pgd) {
		yacht_free_pages((unsigned long)identity_pgd, 0);
		identity_pgd = NULL;
	}
	if (control_page) {
		yacht_free_pages((unsigned long)control_page, 0);
		control_page = NULL;
	}
	if (copy_source) {
		yacht_free_pages((unsigned long)copy_source, 0);
		copy_source = NULL;
	}
	if (copy_destination) {
		yacht_free_pages((unsigned long)copy_destination, 0);
		copy_destination = NULL;
	}
	if (scratch_chunk) {
		yacht_free_pages((unsigned long)scratch_chunk, 0);
		scratch_chunk = NULL;
	}
}

static int yacht_rehearse(void)
{
	phys_addr_t control_pa = yacht_linear_to_phys(control_page);
	phys_addr_t pgd_pa = yacht_linear_to_phys(identity_pgd);
	phys_addr_t entry_pa = control_pa + (yacht_rehearse_entry - yacht_control_start);
	phys_addr_t copy_entry_pa = control_pa +
		(yacht_copy_test_entry - yacht_control_start);
	phys_addr_t list_copy_entry_pa = control_pa +
		(yacht_list_copy_test_entry - yacht_control_start);
	phys_addr_t copy_source_pa = yacht_linear_to_phys(copy_source);
	phys_addr_t copy_destination_pa = yacht_linear_to_phys(copy_destination);
	u64 *marker = control_page + YACHT_MARKER_OFFSET;
	u64 tcr, mair, current_el;
	void __iomem *wdt;
	unsigned int copied_pages = 0;
	unsigned int i;

	asm volatile("mrs %0, tcr_el1" : "=r" (tcr));
	asm volatile("mrs %0, mair_el1" : "=r" (mair));
	asm volatile("mrs %0, CurrentEL" : "=r" (current_el));
	pr_info("yacht_boot: TCR=%llx MAIR=%llx CurrentEL=%llx PGD=%pa control=%pa\n",
		tcr, mair, current_el, &pgd_pa, &control_pa);
	if ((tcr & 0x3f) != 25 || ((tcr >> 14) & 3) != 0 || current_el != 4)
		return -EINVAL;
	if (((mair >> (MT_NORMAL * 8)) & 0xff) != 0xff)
		return -EINVAL;

	*marker = 0;
	yacht_flush_everything();
	yacht_run_identity_test(pgd_pa, entry_pa, control_pa, entry_pa);
	if (*marker != YACHT_MARKER)
		return -EIO;
	pr_info("yacht_boot: identity-map gate OK marker=%llx\n", *marker);

	for (i = 0; i < PAGE_SIZE / sizeof(u64); i++) {
		copy_source[i] = 0x9e3779b97f4a7c15ULL * (i + 1);
		copy_destination[i] = ~copy_source[i];
	}
	yacht_flush_dcache_area(copy_source, PAGE_SIZE);
	yacht_flush_dcache_area(copy_destination, PAGE_SIZE);
	asm volatile("ic iallu; dsb sy; isb" ::: "memory");
	yacht_run_copy_test(pgd_pa, copy_entry_pa,
			    copy_source_pa, copy_destination_pa);
	for (i = 0; i < PAGE_SIZE / sizeof(u64); i++) {
		if (copy_destination[i] != copy_source[i]) {
			pr_err("yacht_boot: MMU-off copy mismatch word=%u got=%llx expected=%llx\n",
			       i, copy_destination[i], copy_source[i]);
			return -EIO;
		}
	}
	pr_info("yacht_boot: exact single-page MMU-off copy gate OK\n");

	/*
	 * Keep TOPRGU as the rehearsal safety net.  Copy at most four pages
	 * per MMU-off visit so interrupts and watchdogd can run between chunks.
	 * The exact single-page gate above exercises low-level invalidation.  This
	 * full scratch-only walk pre-invalidates while all CPUs remain live; the
	 * committed jump stops other CPUs and invalidates in the low loop.
	 */
	wdt = yacht_ioremap(YACHT_WDT_PA, PAGE_SIZE,
			   __pgprot(PROT_DEVICE_nGnRE));
	if (!wdt)
		return -ENOMEM;
	pr_info("yacht_boot: starting pre-invalidated linked-list walk\n");
	for (i = 0; i < scratch_list_pages; i++) {
		unsigned int first;

		for (first = 0; first < scratch_list_virt[i]->count;) {
			unsigned int count = min_t(unsigned int,
				YACHT_REHEARSAL_CHUNK_PAGES,
				scratch_list_virt[i]->count - first);
			unsigned int pair;
			phys_addr_t chunk_pa = yacht_linear_to_phys(scratch_chunk);
			bool invalidate_low = false;

			scratch_chunk->next = 0;
			scratch_chunk->count = count;
			for (pair = 0; pair < count; pair++)
				scratch_chunk->pair[pair] =
					scratch_list_virt[i]->pair[first + pair];
			for (pair = 0; pair < count; pair++)
				yacht_invalidate_dcache_page(
					scratch_pages[copied_pages + pair]);
			yacht_flush_dcache_area(scratch_chunk, PAGE_SIZE);
			asm volatile("ic iallu; dsb sy; isb" ::: "memory");
			yacht_run_list_copy_test(pgd_pa, list_copy_entry_pa,
						 chunk_pa, invalidate_low);
			copied_pages += count;
			__raw_writel(YACHT_WDT_RST_RELOAD, wdt + YACHT_WDT_RST);
			dsb(sy);
			if (!(copied_pages & 63) ||
			    copied_pages == scratch_page_count)
				pr_info("yacht_boot: rehearsal copied %u/%u pages\n",
					copied_pages, scratch_page_count);
			first += count;
		}
	}
	yacht_iounmap(wdt);
	{
		unsigned int page_index = 0;
		unsigned int payload_index;
		for (payload_index = 0; payload_index < ARRAY_SIZE(payloads);
		     payload_index++) {
			unsigned int payload_page;
			for (payload_page = 0;
			     payload_page < payloads[payload_index].page_count;
			     payload_page++, page_index++) {
				size_t offset = (size_t)payload_page * PAGE_SIZE;
				u64 *source = payloads[payload_index].pages[payload_page];
				u64 *destination = scratch_pages[page_index];
				for (i = 0; i < PAGE_SIZE / sizeof(u64); i++) {
					if (destination[i] != source[i]) {
						pr_err("yacht_boot: full copy mismatch payload=%u offset=%zu word=%u\n",
						       payload_index, offset, i);
						return -EIO;
					}
				}
			}
		}
	}
	pr_info("yacht_boot: REHEARSAL OK marker=%llx pairs=%u list_pages=%u\n",
		*marker, ({ unsigned int n = 0, j; for (j = 0; j < list_pages; j++) n += list_virt[j]->count; n; }),
		list_pages);
	pr_info("yacht_boot: MMU-OFF COPY OK source=%pa destination=%pa bytes=%lu\n",
		&copy_source_pa, &copy_destination_pa, PAGE_SIZE);
	pr_info("yacht_boot: FULL MMU-OFF COPY OK pages=%u bytes=%llu\n",
		scratch_page_count, (u64)scratch_page_count * PAGE_SIZE);
	yacht_free_scratch();
	return 0;
}

static void yacht_jump(void)
{
	phys_addr_t control_pa = yacht_linear_to_phys(control_page);
	phys_addr_t pgd_pa = yacht_linear_to_phys(identity_pgd);
	phys_addr_t jump_pa = control_pa + (yacht_jump_entry - yacht_control_start);
	void __iomem *wdt;
	u32 mode;
	int error;

	wdt = yacht_ioremap(YACHT_WDT_PA, PAGE_SIZE, __pgprot(PROT_DEVICE_nGnRE));
	if (!wdt) {
		pr_err("yacht_boot: could not map TOPRGU\n");
		return;
	}
	yacht_flush_everything();
	pr_emerg("yacht_boot: COMMIT RAM-ONLY RECOVERY JUMP\n");
	yacht_aee_rr_rec_last_init_func(YACHT_PHASE_BASE | 1);
	yacht_kernel_restart_prepare("yacht recovery");
	yacht_aee_rr_rec_last_init_func(YACHT_PHASE_BASE | 2);
	yacht_migrate_to_reboot_cpu();
	yacht_aee_rr_rec_last_init_func(YACHT_PHASE_BASE | 3);

	/*
	 * Yacht's vendor CPU-hotplug path wedges inside cpu_down().  Dispatch raw
	 * PSCI CPU_OFF concurrently instead; firmware powers down five cores, and
	 * the two known rejectors stay masked and parked in the callback forever.
	 */
	pr_emerg("yacht_boot: dispatching direct PSCI off-or-park fanout\n");
	error = yacht_quiesce_secondaries();
	if (error) {
		pr_emerg("yacht_boot: secondary fanout failed=%d; rebooting\n",
			 error);
		yacht_emergency_restart();
	}
	pr_emerg("yacht_boot: secondary CPUs are off or permanently parked\n");
	yacht_aee_rr_rec_last_init_func(YACHT_PHASE_BASE | 4);

	/*
	 * Leave a hardware escape hatch across the MMU-off relocation and early
	 * recovery boot.  Match MediaTek's direct-reset watchdog test mode: no
	 * IRQ-first or debug/dual stage, so a frozen jump resets after 30 seconds.
	 * A healthy target kernel is built with AMAZON_BOOTUP_KEEP_WATCHDOG and
	 * will reprogram/ping TOPRGU during probe before userspace watchdogd runs.
	 */
	__raw_writel((YACHT_WDT_TIMEOUT_SECONDS << 11) |
		       YACHT_WDT_LENGTH_KEY, wdt + YACHT_WDT_LENGTH);
	__raw_writel(YACHT_WDT_RST_RELOAD, wdt + YACHT_WDT_RST);
	mode = __raw_readl(wdt + YACHT_WDT_MODE);
	mode &= ~(YACHT_WDT_EXT_POL_HIGH | YACHT_WDT_IRQ_ENABLE |
		  YACHT_WDT_DUAL_ENABLE);
	mode |= YACHT_WDT_ENABLE | YACHT_WDT_EXRST_ENABLE |
		YACHT_WDT_AUTO_START | YACHT_WDT_IRQ_LEVEL | YACHT_WDT_KEY;
	__raw_writel(mode, wdt + YACHT_WDT_MODE);
	dsb(sy);
	pr_emerg("yacht_boot: TOPRGU armed direct-reset timeout=%us mode=%08x length=%08x\n",
		 YACHT_WDT_TIMEOUT_SECONDS,
		 __raw_readl(wdt + YACHT_WDT_MODE),
		 __raw_readl(wdt + YACHT_WDT_LENGTH));
	yacht_aee_rr_rec_last_init_func(YACHT_PHASE_BASE | 5);
	asm volatile("ic iallu; dsb sy; isb" ::: "memory");
	yacht_aee_rr_rec_last_init_func(YACHT_PHASE_BASE | 6);
	yacht_enter_jump(pgd_pa, jump_pa, first_list_pa,
			 YACHT_IMAGE_PA, YACHT_DTB_PA);
}

static int __init yacht_boot_init(void)
{
	unsigned int i;
	int error;

	pr_info("yacht_boot: mode=%s (no block writes in module)\n",
		YACHT_JUMP_MODE ? "JUMP" : "REHEARSAL");
	BUILD_BUG_ON(sizeof(struct yacht_list_page) != PAGE_SIZE);
	error = yacht_resolve();
	if (error)
		return error;
	{
		void __iomem *wdt = yacht_ioremap(YACHT_WDT_PA, PAGE_SIZE,
						 __pgprot(PROT_DEVICE_nGnRE));
		if (wdt) {
			pr_info("yacht_boot: TOPRGU mode=%08x nonrst=%08x nonrst2=%08x phase=%04x\n",
				__raw_readl(wdt), __raw_readl(wdt + 0x20),
				__raw_readl(wdt + YACHT_WDT_NONRST2),
				__raw_readl(wdt + YACHT_WDT_NONRST2) >> 16);
			yacht_iounmap(wdt);
		}
	}
	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		error = yacht_read_file(&payloads[i]);
		if (error)
			goto fail;
	}
	error = yacht_validate_content();
	if (error)
		goto fail;
	error = yacht_build_copy_list();
	if (error)
		goto fail;
	error = yacht_build_scratch_list();
	if (error)
		goto fail;
	error = yacht_prepare_control();
	if (error)
		goto fail;
	yacht_release_quarantine();
	error = yacht_rehearse();
	if (error)
		goto fail;
	if (YACHT_JUMP_MODE) {
		yacht_jump();
		return -EIO;
	}
	return 0;

fail:
	pr_err("yacht_boot: preparation failed: %d\n", error);
	yacht_cleanup();
	return error;
}

static void __exit yacht_boot_exit(void)
{
	yacht_cleanup();
	pr_info("yacht_boot: rehearsal allocations freed\n");
}

module_init(yacht_boot_init);
module_exit(yacht_boot_exit);
MODULE_DESCRIPTION("RAM-only recovery warm boot for Amazon yacht");
MODULE_AUTHOR("ai_re");
MODULE_LICENSE("GPL");
