// SPDX-License-Identifier: GPL-2.0
/* Non-jumping recovery payload staging probe for Amazon yacht. */
#include <linux/err.h>
#include <linux/fs.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/mm.h>
#include <linux/module.h>
#include <linux/uaccess.h>
#include <linux/vmalloc.h>

extern unsigned long kallsyms_lookup_name(const char *name);

struct yacht_payload {
	const char *name;
	const char *path;
	size_t size;
	phys_addr_t destination;
	void *data;
};

static struct yacht_payload payloads[] = {
	{
		.name = "Image",
		.path = "/mnt/yacht-stage/Image",
		.size = 25932040,
		.destination = 0x40080000,
	},
	{
		.name = "ramdisk.gz",
		.path = "/mnt/yacht-stage/ramdisk.gz",
		.size = 15397617,
		.destination = 0x55000000,
	},
	{
		.name = "yacht-live-recovery.dtb",
		.path = "/mnt/yacht-stage/yacht-live-recovery.dtb",
		.size = 215203,
		.destination = 0x54000000,
	},
};

static void *(*yacht_vmalloc)(unsigned long size);
static void (*yacht_vfree)(const void *addr);
static unsigned long (*yacht_vmalloc_to_pfn)(const void *addr);
static struct file *(*yacht_filp_open)(const char *filename, int flags,
				      umode_t mode);
static int (*yacht_filp_close)(struct file *filp, fl_owner_t id);
static ssize_t (*yacht_vfs_read)(struct file *file, char __user *buf,
				 size_t count, loff_t *pos);

static int yacht_resolve(void)
{
#define RESOLVE(name) do { \
	yacht_##name = (void *)kallsyms_lookup_name(#name); \
	if (!yacht_##name) { \
		pr_err("yacht_stage: missing %s\n", #name); \
		return -ENOENT; \
	} \
} while (0)
	RESOLVE(vmalloc);
	RESOLVE(vfree);
	RESOLVE(vmalloc_to_pfn);
	RESOLVE(filp_open);
	RESOLVE(filp_close);
	RESOLVE(vfs_read);
#undef RESOLVE
	return 0;
}

static int yacht_read_file(struct yacht_payload *payload)
{
	struct file *file;
	mm_segment_t old_fs;
	loff_t position = 0;
	ssize_t count;
	size_t done = 0;

	file = yacht_filp_open(payload->path, O_RDONLY, 0);
	if (IS_ERR(file))
		return PTR_ERR(file);

	payload->data = yacht_vmalloc(payload->size);
	if (!payload->data) {
		yacht_filp_close(file, NULL);
		return -ENOMEM;
	}

	old_fs = get_fs();
	set_fs(KERNEL_DS);
	while (done < payload->size) {
		count = yacht_vfs_read(file, payload->data + done,
				       payload->size - done, &position);
		if (count <= 0)
			break;
		done += count;
	}
	set_fs(old_fs);
	yacht_filp_close(file, NULL);

	if (done != payload->size) {
		pr_err("yacht_stage: %s short read %zu/%zu\n",
		       payload->name, done, payload->size);
		return -EIO;
	}
	return 0;
}

static bool yacht_destination_overlap(phys_addr_t page)
{
	size_t i;

	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		phys_addr_t start = payloads[i].destination;
		phys_addr_t end = start + payloads[i].size;

		if (page < end && page + PAGE_SIZE > start)
			return true;
	}
	return false;
}

static int yacht_validate_pages(struct yacht_payload *payload)
{
	size_t offset;
	unsigned long min_pfn = ~0UL;
	unsigned long max_pfn = 0;
	unsigned long overlaps = 0;
	unsigned long pages = 0;

	for (offset = 0; offset < payload->size; offset += PAGE_SIZE) {
		unsigned long pfn = yacht_vmalloc_to_pfn(payload->data + offset);
		phys_addr_t physical = (phys_addr_t)pfn << PAGE_SHIFT;

		if (pfn < min_pfn)
			min_pfn = pfn;
		if (pfn > max_pfn)
			max_pfn = pfn;
		if (yacht_destination_overlap(physical))
			overlaps++;
		pages++;
	}

	pr_info("yacht_stage: %s pages=%lu pfn=%lx..%lx destination=%pa overlaps=%lu\n",
		payload->name, pages, min_pfn, max_pfn, &payload->destination,
		overlaps);
	return overlaps ? -EADDRINUSE : 0;
}

static int yacht_validate_content(void)
{
	const unsigned char *image = payloads[0].data;
	const unsigned char *ramdisk = payloads[1].data;
	const unsigned char *dtb = payloads[2].data;
	u32 dtb_size;

	if (image[0x38] != 'A' || image[0x39] != 'R' ||
	    image[0x3a] != 'M' || image[0x3b] != 0x64)
		return -ENOEXEC;
	if (ramdisk[0] != 0x1f || ramdisk[1] != 0x8b)
		return -ENOEXEC;
	if (dtb[0] != 0xd0 || dtb[1] != 0x0d ||
	    dtb[2] != 0xfe || dtb[3] != 0xed)
		return -ENOEXEC;
	dtb_size = ((u32)dtb[4] << 24) | ((u32)dtb[5] << 16) |
		   ((u32)dtb[6] << 8) | dtb[7];
	if (dtb_size > payloads[2].size || dtb_size < 40)
		return -EINVAL;

	pr_info("yacht_stage: content valid: arm64 Image, gzip ramdisk, FDT size=%u\n",
		dtb_size);
	return 0;
}

static void yacht_free_payloads(void)
{
	size_t i;

	if (!yacht_vfree)
		return;
	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		if (payloads[i].data) {
			yacht_vfree(payloads[i].data);
			payloads[i].data = NULL;
		}
	}
}

static int __init yacht_stage_init(void)
{
	size_t i;
	int error;

	pr_info("yacht_stage: non-jumping recovery staging probe\n");
	error = yacht_resolve();
	if (error)
		return error;

	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		error = yacht_read_file(&payloads[i]);
		if (error)
			goto fail;
	}
	error = yacht_validate_content();
	if (error)
		goto fail;
	for (i = 0; i < ARRAY_SIZE(payloads); i++) {
		error = yacht_validate_pages(&payloads[i]);
		if (error)
			goto fail;
	}

	pr_info("yacht_stage: STAGED OK; no destination write or branch performed\n");
	return 0;

fail:
	pr_err("yacht_stage: validation failed: %d\n", error);
	yacht_free_payloads();
	return error;
}

static void __exit yacht_stage_exit(void)
{
	yacht_free_payloads();
	pr_info("yacht_stage: freed all staging pages\n");
}

module_init(yacht_stage_init);
module_exit(yacht_stage_exit);

MODULE_DESCRIPTION("Non-jumping recovery payload staging probe for Amazon yacht");
MODULE_AUTHOR("ai_re");
MODULE_LICENSE("GPL");
