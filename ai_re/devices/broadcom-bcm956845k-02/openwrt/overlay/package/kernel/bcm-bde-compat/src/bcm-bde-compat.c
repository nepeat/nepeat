// SPDX-License-Identifier: GPL-2.0-only
/* Device-specific implementation of the legacy 32-bit BDE userspace ABI.
 * No vendor driver implementation is included. The recovered SDK accesses
 * /dev/linux-user-bde for ioctls and /dev/bcm for bounded memory mappings.
 */
#include <linux/module.h>
#include <linux/pci.h>
#include <linux/miscdevice.h>
#include <linux/fs.h>
#include <linux/mm.h>
#include <linux/dma-mapping.h>
#include <linux/of_address.h>
#include <linux/interrupt.h>
#include <linux/wait.h>
#include <linux/delay.h>
#include <linux/uaccess.h>
#include <linux/capability.h>
#include <linux/sizes.h>
#include <linux/idr.h>
#include <linux/refcount.h>

#define DRIVER "bcm-bde-compat"
#define DMA_BYTES SZ_16M
#define CMICM_STAT0 0x31400
#define CMICM_STAT1 0x31404
#define CMICM_MASK0 0x31414
#define CMICM_MASK1 0x31418

struct bde_ioctl {
	u32 dev, rc, d0, d1, d2, d3, p0;
	u8 data[64];
};

struct bde_semaphore {
	wait_queue_head_t wait;
	atomic_t count;
	refcount_t references;
	bool binary, dead;
};

struct bde_device {
	struct pci_dev *pci;
	void __iomem *bar;
	resource_size_t bar_start, bar_size;
	void __iomem *dma_cpu;
	dma_addr_t dma_bus;
	bool irq_enabled;
	struct mutex irq_lock;
	wait_queue_head_t wait;
	atomic_t pending;
	struct idr semaphores;
	struct mutex semaphore_lock;
	struct miscdevice control, memory;
};

static struct bde_device *active;
static bool trace;
module_param(trace, bool, 0600);
MODULE_PARM_DESC(trace, "Log legacy ioctl requests for ABI bring-up");

static void put_semaphore(struct bde_semaphore *sem)
{
	if (refcount_dec_and_test(&sem->references))
		kfree(sem);
}

static int semaphore_operation(struct bde_device *b, struct bde_ioctl *io)
{
	struct bde_semaphore *sem;
	int handle, error = 0;
	long result;

	if (io->d0 == 1) {
		if (io->d2 > 1000000)
			return -EINVAL;
		sem = kzalloc(sizeof(*sem), GFP_KERNEL);
		if (!sem)
			return -ENOMEM;
		init_waitqueue_head(&sem->wait);
		sem->binary = !!io->d1;
		atomic_set(&sem->count, sem->binary ? !!io->d2 : io->d2);
		refcount_set(&sem->references, 1);
		mutex_lock(&b->semaphore_lock);
		handle = idr_alloc(&b->semaphores, sem, 1, 65536, GFP_KERNEL);
		mutex_unlock(&b->semaphore_lock);
		io->p0 = handle < 0 ? 0 : handle;
		if (handle < 0) {
			put_semaphore(sem);
			return handle;
		}
		return 0;
	}
	mutex_lock(&b->semaphore_lock);
	sem = idr_find(&b->semaphores, io->p0);
	if (sem) {
		if (io->d0 == 2)
			idr_remove(&b->semaphores, io->p0);
		else
			refcount_inc(&sem->references);
	}
	mutex_unlock(&b->semaphore_lock);
	if (!sem)
		return -EINVAL;
	switch (io->d0) {
	case 2:
		WRITE_ONCE(sem->dead, true);
		wake_up_interruptible_all(&sem->wait);
		break;
	case 3:
		if (io->d2 == U32_MAX) {
			error = wait_event_interruptible(sem->wait,
				READ_ONCE(sem->dead) || atomic_dec_if_positive(&sem->count) >= 0);
		} else if (!io->d2) {
			error = atomic_dec_if_positive(&sem->count) < 0 ? -EAGAIN : 0;
		} else {
			result = wait_event_interruptible_timeout(sem->wait,
				READ_ONCE(sem->dead) || atomic_dec_if_positive(&sem->count) >= 0,
				max_t(unsigned long, 1, usecs_to_jiffies(io->d2)));
			error = result > 0 ? 0 : result < 0 ? result : -ETIMEDOUT;
		}
		if (READ_ONCE(sem->dead))
			error = -EIDRM;
		break;
	case 4:
		if (sem->binary)
			atomic_cmpxchg(&sem->count, 0, 1);
		else
			atomic_inc(&sem->count);
		wake_up_interruptible(&sem->wait);
		break;
	default:
		error = -EINVAL;
	}
	put_semaphore(sem);
	return error;
}

static void mask_interrupts(struct bde_device *b)
{
	/* Zero is invariant under the PCI PIO endian mode selected by the SDK. */
	iowrite32(0, b->bar + CMICM_MASK0);
	iowrite32(0, b->bar + CMICM_MASK1);
	(void)ioread32(b->bar + CMICM_MASK0);
}

static irqreturn_t bde_interrupt(int irq, void *context)
{
	struct bde_device *b = context;
	u32 pending;

	pending = ioread32(b->bar + CMICM_STAT0) &
		   ioread32(b->bar + CMICM_MASK0);
	pending |= ioread32(b->bar + CMICM_STAT1) &
		   ioread32(b->bar + CMICM_MASK1);
	if (!pending)
		return IRQ_NONE;
	mask_interrupts(b);
	atomic_set(&b->pending, 1);
	wake_up_interruptible(&b->wait);
	return IRQ_HANDLED;
}

static int change_interrupts(struct bde_device *b, bool enable)
{
	int error = 0;

	mutex_lock(&b->irq_lock);
	if (enable && !b->irq_enabled) {
		atomic_set(&b->pending, 0);
		error = request_irq(b->pci->irq, bde_interrupt, IRQF_SHARED, DRIVER, b);
		if (!error)
			WRITE_ONCE(b->irq_enabled, true);
	} else if (!enable && b->irq_enabled) {
		mask_interrupts(b);
		WRITE_ONCE(b->irq_enabled, false);
		free_irq(b->pci->irq, b);
		wake_up_interruptible(&b->wait);
	}
	mutex_unlock(&b->irq_lock);
	return error;
}

static long bde_ioctl(struct file *file, unsigned int command, unsigned long argument)
{
	struct bde_device *b = READ_ONCE(active);
	struct bde_ioctl io;
	void __user *user = (void __user *)argument;
	unsigned int op = _IOC_NR(command);
	int error = 0;
	u32 value;

	if (!capable(CAP_SYS_RAWIO))
		return -EPERM;
	if (!b)
		return -ENODEV;
	if (_IOC_TYPE(command) != 'L' || command != _IO('L', op))
		return -ENOTTY;
	if (copy_from_user(&io, user, sizeof(io)))
		return -EFAULT;
	if (trace)
		pr_info_ratelimited(DRIVER ": ioctl %u dev=%u d0=%x d1=%x d2=%x\n",
				    op, io.dev, io.d0, io.d1, io.d2);
	io.rc = 0;
	/* The legacy SDK leaves dev uninitialized for global sleep/semaphore
	 * operations; only device-specific requests may validate that field.
	 */
	if (io.dev && op != 0 && op != 1 && op != 8 && op != 10 && op != 11) {
		io.rc = U32_MAX;
		goto copy;
	}
	switch (op) {
	case 0: /* Version: the recovered driver predates versioned extensions. */
		io.d0 = 0;
		break;
	case 1: /* Device counts: all, switch, Ethernet, CPU. */
		io.d0 = io.dev <= 1 ? 1 : 0;
		break;
	case 2:
		io.d0 = b->pci->device;
		io.d1 = b->pci->revision;
		io.d2 = lower_32_bits(b->bar_start);
		io.d3 = upper_32_bits(b->bar_start);
		break;
	case 3:
		if ((io.d0 & 3) || io.d0 > 252 ||
		    (io.d0 >= PCI_BASE_ADDRESS_0 && io.d0 <= PCI_BASE_ADDRESS_5)) {
			error = -EINVAL;
			break;
		}
		error = pci_write_config_dword(b->pci, io.d0, io.d1);
		break;
	case 4:
		if ((io.d0 & 3) || io.d0 > 252) {
			error = -EINVAL;
			break;
		}
		error = pci_read_config_dword(b->pci, io.d0, &value);
		io.d0 = value;
		break;
	case 5:
		io.d0 = lower_32_bits(b->dma_bus);
		io.d1 = DMA_BYTES;
		break;
	case 6:
	case 7:
		error = change_interrupts(b, op == 6);
		break;
	case 8:
	case 11:
		if (io.d0 > 10000000) {
			error = -EINVAL;
			break;
		}
		if (op == 11 && io.d0 <= 1000)
			udelay(io.d0);
		else if (io.d0)
			usleep_range(io.d0, io.d0 + 100);
		break;
	case 9:
		error = wait_event_interruptible(b->wait,
			atomic_xchg(&b->pending, 0) || !READ_ONCE(b->irq_enabled));
		if (error)
			return error;
		break;
	case 10:
		error = semaphore_operation(b, &io);
		break;
	case 12:
		io.d0 = 0x20000101; /* PCI + switch + 256 KiB BAR. */
		break;
	case 21:
		io.d0 = io.d1 = io.d2 = 0; /* This SDK uses its compiled XGS bus flags. */
		break;
	case 22:
		if ((io.d0 & 3) || io.d0 > b->bar_size - 4) {
			error = -EINVAL;
			break;
		}
		/* Userspace register accesses are native big-endian stores. */
		iowrite32be(io.d1, b->bar + io.d0);
		break;
	default:
		pr_warn_ratelimited(DRIVER ": unsupported ioctl %u\n", op);
		error = -EOPNOTSUPP;
	}
	if (error)
		io.rc = U32_MAX;
copy:
	return copy_to_user(user, &io, sizeof(io)) ? -EFAULT : 0;
}

static int bde_mmap(struct file *file, struct vm_area_struct *vma)
{
	struct bde_device *b = READ_ONCE(active);
	u64 physical = (u64)vma->vm_pgoff << PAGE_SHIFT;
	unsigned long length = vma->vm_end - vma->vm_start;

	if (!capable(CAP_SYS_RAWIO))
		return -EPERM;
	if (!b)
		return -ENODEV;
	if (!(vma->vm_flags & VM_SHARED))
		return -EINVAL;
	if (physical >= b->bar_start && physical - b->bar_start <= b->bar_size &&
	    length <= b->bar_size - (physical - b->bar_start)) {
		vm_flags_set(vma, VM_IO | VM_DONTEXPAND | VM_DONTDUMP);
		vma->vm_page_prot = pgprot_noncached(vma->vm_page_prot);
		return remap_pfn_range(vma, vma->vm_start, physical >> PAGE_SHIFT,
				      length, vma->vm_page_prot);
	}
	if (physical >= b->dma_bus && physical - b->dma_bus <= DMA_BYTES &&
	    length <= DMA_BYTES - (physical - b->dma_bus)) {
		vm_flags_set(vma, VM_IO | VM_DONTEXPAND | VM_DONTDUMP);
		vma->vm_page_prot = pgprot_noncached(vma->vm_page_prot);
		return remap_pfn_range(vma, vma->vm_start, physical >> PAGE_SHIFT,
				      length, vma->vm_page_prot);
	}
	return -EINVAL;
}

static const struct file_operations bde_operations = {
	.owner = THIS_MODULE,
	.unlocked_ioctl = bde_ioctl,
	.mmap = bde_mmap,
};

static int bde_probe(struct pci_dev *pci, const struct pci_device_id *id)
{
	struct bde_device *b;
	struct device_node *node;
	struct resource pool;
	int error;

	BUILD_BUG_ON(sizeof(struct bde_ioctl) != 92);
	if (active)
		return -EBUSY;
	b = devm_kzalloc(&pci->dev, sizeof(*b), GFP_KERNEL);
	if (!b)
		return -ENOMEM;
	b->pci = pci;
	mutex_init(&b->irq_lock);
	init_waitqueue_head(&b->wait);
	atomic_set(&b->pending, 0);
	idr_init(&b->semaphores);
	mutex_init(&b->semaphore_lock);
	error = pcim_enable_device(pci);
	if (error)
		return error;
	error = dma_set_mask_and_coherent(&pci->dev, DMA_BIT_MASK(32));
	if (error)
		return error;
	b->bar_start = pci_resource_start(pci, 0);
	b->bar_size = pci_resource_len(pci, 0);
	if (!(pci_resource_flags(pci, 0) & IORESOURCE_MEM) || b->bar_size != SZ_256K)
		return -EINVAL;
	error = pci_request_region(pci, 0, DRIVER);
	if (error)
		return error;
	b->bar = pci_iomap(pci, 0, 0);
	if (!b->bar) {
		error = -ENOMEM;
		goto release_bar;
	}
	node = of_find_compatible_node(NULL, NULL, "emma,bcm56845-dma-pool");
	if (!node) {
		error = -ENODEV;
		goto unmap_bar;
	}
	error = of_address_to_resource(node, 0, &pool);
	of_node_put(node);
	if (error || resource_size(&pool) < DMA_BYTES) {
		error = -EINVAL;
		goto unmap_bar;
	}
	/* This board has a 1:1 PCI DMA window for its first 512 MiB. Its DTB
	 * removes this fixed pool from normal RAM, so both kernel and SDK use
	 * uncached mappings. No private, unexported coherent-pool API is needed.
	 */
	if (pool.start != 0x1c000000 || resource_size(&pool) != SZ_32M) {
		error = -EINVAL;
		goto unmap_bar;
	}
	b->dma_bus = pool.start;
	b->dma_cpu = ioremap(pool.start, DMA_BYTES);
	if (!b->dma_cpu) {
		error = -ENOMEM;
		goto unmap_bar;
	}
	memset_io(b->dma_cpu, 0, DMA_BYTES);
	wmb();
	mask_interrupts(b);
	b->control = (struct miscdevice) {
		.minor = MISC_DYNAMIC_MINOR, .name = "linux-user-bde",
		.fops = &bde_operations, .mode = 0600,
	};
	b->memory = (struct miscdevice) {
		.minor = MISC_DYNAMIC_MINOR, .name = "bcm",
		.fops = &bde_operations, .mode = 0600,
	};
	WRITE_ONCE(active, b);
	error = misc_register(&b->control);
	if (error)
		goto clear_active;
	error = misc_register(&b->memory);
	if (error)
		goto deregister_control;
	pci_set_master(pci);
	pci_set_drvdata(pci, b);
	dev_info(&pci->dev, "legacy BDE: BAR %pa, DMA %pad (%u bytes), IRQ %u\n",
		 &b->bar_start, &b->dma_bus, DMA_BYTES, pci->irq);
	return 0;
deregister_control:
	misc_deregister(&b->control);
clear_active:
	WRITE_ONCE(active, NULL);
	iounmap(b->dma_cpu);
unmap_bar:
	pci_iounmap(pci, b->bar);
release_bar:
	pci_release_region(pci, 0);
	return error;
}

static void bde_remove(struct pci_dev *pci)
{
	struct bde_device *b = pci_get_drvdata(pci);
	struct bde_semaphore *sem;
	int handle;

	misc_deregister(&b->memory);
	misc_deregister(&b->control);
	change_interrupts(b, false);
	pci_clear_master(pci);
	WRITE_ONCE(active, NULL);
	idr_for_each_entry(&b->semaphores, sem, handle)
		put_semaphore(sem);
	idr_destroy(&b->semaphores);
	iounmap(b->dma_cpu);
	pci_iounmap(pci, b->bar);
	pci_release_region(pci, 0);
}

static const struct pci_device_id bde_devices[] = {
	{ PCI_DEVICE(0x14e4, 0xb845) },
	{ }
};
MODULE_DEVICE_TABLE(pci, bde_devices);

static struct pci_driver bde_driver = {
	.name = DRIVER, .id_table = bde_devices,
	.probe = bde_probe, .remove = bde_remove,
};
module_pci_driver(bde_driver);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("BCM56845 legacy userspace SDK compatibility for OpenWrt");
