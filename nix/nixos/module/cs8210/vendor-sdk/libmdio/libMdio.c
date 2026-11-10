// SPDX-License-Identifier: GPL-2.0
/*
 * libMdio.so replacement for the Celestica CS8210-32X Credo PHY tooling.
 *
 * The vendor c-phy/libMdio.so reaches the 16 Credo CMS50216 MDIO ports
 * through /dev/mdio-N:XX ioctls and /sys/bus/mdio_bus/devices/mdio-N:XX/
 * attributes, both provided by vendor 4.9-only kernel modules (mdio-fpga,
 * mdio-lattice, cms50216). This drop-in drives the MDIO master FPGA
 * (PCI 1d0f:ec30, BAR0) directly from userspace instead, so phy_server /
 * libcredo run on a kernel that has none of those modules.
 *
 * Same exported interface as the vendor library (recovered by disassembly):
 *
 *   int PHY_mdio_init(void);
 *   int PHY_mdio_read(uint8_t chip, uint8_t sub, uint16_t reg, uint16_t *val);
 *   int PHY_mdio_write(uint8_t chip, uint8_t sub, uint16_t reg, uint16_t val);
 *   int PHY_mdc_freq_set(uint8_t chip, uint8_t sub, int hz);
 *   int mdio_read_ioctl(uint8_t phy, uint16_t reg, uint16_t *val);   (+ _sysfs)
 *   int mdio_write_ioctl(uint8_t phy, uint16_t reg, uint16_t val);   (+ _sysfs)
 *   int mdc_freq_set(uint8_t phy, int hz);
 *
 * chip 0..7, sub 0..1 -> PHY address 2 + chip*2 + sub; PHY address A sits
 * alone on MDIO master A-1. Every access is clause 45, MMD 1, like the
 * vendor cms50216 driver does it.
 *
 * FPGA register use follows mdio-lattice.ko 1.5 ("new" completion
 * handshake, selected by the vendor when system CPLD reg 0x01 has a
 * non-zero high nibble; this unit reads 0x10):
 *
 *   master N block at BAR0 + 0xc0000 + 0x100*(N-1), 16-bit registers
 *     +0x0  devad | phy << 5
 *     +0x4  register address
 *     +0x8  write data / read result
 *     +0xc  control: bit0 enable, bit2 start, bit3 read, bit5 error,
 *           bits 15:8 MDC divider
 *   BAR0 + 0x40000  done status, bit N-1 = master N   (16-bit)
 *   BAR0 + 0x40004  write-1-to-acknowledge            (16-bit)
 *
 * Only one process may use this at a time (phy_server is the single
 * user); a flock on the resource file enforces it across processes and a
 * mutex within one.
 */
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/file.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#define FPGA_VENDOR 0x1d0f
#define FPGA_DEVICE 0xec30
#define BAR0_SIZE 0x200000

#define REG_VERSION 0x000000
#define REG_IRQ_STATUS 0x040000
#define REG_IRQ_ACK 0x040004
#define MASTER_BASE(n) (0x0c0000 + 0x100 * ((n) - 1))
#define M_ADDR 0x0
#define M_REG 0x4
#define M_DATA 0x8
#define M_CTRL 0xc

#define CTRL_ENABLE 0x01
#define CTRL_START 0x04
#define CTRL_READ 0x08
#define CTRL_ERROR 0x20

#define MDC_DIV_DEFAULT 0x7d
#define NUM_MASTERS 16
#define DEVAD 1
#define TIMEOUT_NS 500000000LL /* vendor: wait_for_completion_timeout(.., 500) */

static volatile uint8_t *bar;
static uint8_t mdc_div[NUM_MASTERS + 1];
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static inline uint16_t r16(uint32_t off)
{
	return *(volatile uint16_t *)(bar + off);
}

static inline void w16(uint32_t off, uint16_t v)
{
	*(volatile uint16_t *)(bar + off) = v;
}

static inline uint32_t r32(uint32_t off)
{
	return *(volatile uint32_t *)(bar + off);
}

static long long now_ns(void)
{
	struct timespec ts;

	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static void delay_us(unsigned int us)
{
	long long end = now_ns() + us * 1000LL;

	while (now_ns() < end)
		;
}

static int read_hex_file(const char *path, unsigned int *out)
{
	char buf[32];
	unsigned int v = 0;
	int fd, n, i;

	fd = open(path, O_RDONLY);
	if (fd < 0)
		return -1;
	n = read(fd, buf, sizeof(buf) - 1);
	close(fd);
	if (n <= 0)
		return -1;
	for (i = 0; i < n; i++) {
		char c = buf[i];

		if (c == 'x' || c == 'X') {
			v = 0;
			continue;
		}
		if (c >= '0' && c <= '9')
			v = v * 16 + (c - '0');
		else if (c >= 'a' && c <= 'f')
			v = v * 16 + (c - 'a' + 10);
		else if (c >= 'A' && c <= 'F')
			v = v * 16 + (c - 'A' + 10);
		else
			break;
	}
	*out = v;
	return 0;
}

static int fpga_map(void)
{
	static const char *root = "/sys/bus/pci/devices";
	char path[512];
	struct dirent *de;
	DIR *d;
	int fd = -1;
	int i;

	if (bar)
		return 0;

	d = opendir(root);
	if (!d) {
		fprintf(stderr, "libMdio: cannot list %s: %s\n", root, strerror(errno));
		return -1;
	}
	while ((de = readdir(d))) {
		unsigned int ven = 0, dev = 0;

		if (de->d_name[0] == '.')
			continue;
		snprintf(path, sizeof(path), "%s/%s/vendor", root, de->d_name);
		if (read_hex_file(path, &ven) || ven != FPGA_VENDOR)
			continue;
		snprintf(path, sizeof(path), "%s/%s/device", root, de->d_name);
		if (read_hex_file(path, &dev) || dev != FPGA_DEVICE)
			continue;

		/*
		 * The FPGA leaves U-Boot with PCI memory decode off; a BAR
		 * access in that state is not acknowledged and takes the
		 * machine down with an SError. Enable it first.
		 */
		snprintf(path, sizeof(path), "%s/%s/enable", root, de->d_name);
		fd = open(path, O_WRONLY);
		if (fd >= 0) {
			if (write(fd, "1", 1) != 1)
				fprintf(stderr, "libMdio: enabling %s: %s\n", de->d_name, strerror(errno));
			close(fd);
		}
		snprintf(path, sizeof(path), "%s/%s/resource0", root, de->d_name);
		fd = open(path, O_RDWR | O_SYNC);
		break;
	}
	closedir(d);

	if (fd < 0) {
		fprintf(stderr, "libMdio: MDIO FPGA %04x:%04x resource0 not available\n", FPGA_VENDOR, FPGA_DEVICE);
		return -1;
	}
	if (flock(fd, LOCK_EX | LOCK_NB)) {
		fprintf(stderr, "libMdio: MDIO FPGA is in use by another process\n");
		close(fd);
		return -1;
	}
	bar = mmap(NULL, BAR0_SIZE, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	if (bar == MAP_FAILED) {
		fprintf(stderr, "libMdio: mmap BAR0: %s\n", strerror(errno));
		bar = NULL;
		close(fd);
		return -1;
	}
	/* fd stays open on purpose: it holds the flock for the process lifetime. */

	for (i = 1; i <= NUM_MASTERS; i++) {
		mdc_div[i] = MDC_DIV_DEFAULT;
		/* lattice_mdio_reset(): hold in reset, 100 us, release. */
		w16(MASTER_BASE(i) + M_CTRL, 0);
		delay_us(100);
		w16(MASTER_BASE(i) + M_CTRL, CTRL_ENABLE);
	}
	/* Drop any stale completion bits. */
	w16(REG_IRQ_ACK, 0xffff);
	return 0;
}

static int master_wait(int master)
{
	uint16_t bit = 1u << (master - 1);
	long long end = now_ns() + TIMEOUT_NS;

	do {
		if (r16(REG_IRQ_STATUS) & bit) {
			w16(REG_IRQ_ACK, bit);
			return 0;
		}
	} while (now_ns() < end);
	return -1;
}

/*
 * The vendor command files change the MDC clock with shell lines such as
 *   echo 20000000 > /sys/bus/mdio_bus/devices/mdio-1:02/mdc_freq
 * run by phy_shell, i.e. from another process. The chroot launcher provides
 * those paths as plain files, so the frequency is picked up from there
 * before each transfer. A missing or unparsable file keeps the last value.
 * Same four frequencies and divider values as cms50216.ko's mdc_freq
 * attribute (1/10/20/30 MHz -> 0x7c/0x0c/0x06/0x04).
 */
static uint8_t hz_to_div(long hz, uint8_t fallback)
{
	switch (hz) {
	case 1000000:
		return 0x7c;
	case 10000000:
		return 0x0c;
	case 20000000:
		return 0x06;
	case 30000000:
		return 0x04;
	default:
		return fallback;
	}
}

static void refresh_div(int master, uint8_t phy)
{
	char path[96], buf[24];
	long hz = 0;
	int fd, n, i;

	snprintf(path, sizeof(path), "/sys/bus/mdio_bus/devices/mdio-%d:%02x/mdc_freq", master, phy);
	fd = open(path, O_RDONLY);
	if (fd < 0)
		return;
	n = read(fd, buf, sizeof(buf) - 1);
	close(fd);
	for (i = 0; i < n && buf[i] >= '0' && buf[i] <= '9'; i++)
		hz = hz * 10 + (buf[i] - '0');
	mdc_div[master] = hz_to_div(hz, mdc_div[master]);
}

static int phy_to_master(uint8_t phy)
{
	int master = phy - 1;

	return (master >= 1 && master <= NUM_MASTERS) ? master : -1;
}

/*
 * Optional access trace, for comparing against other PHY stacks: if
 * TRACE_PATH exists when the process makes its first access, every
 * transfer is appended to it as "t_ns R|W phy reg val ret". Create the
 * file to enable, delete it (and restart phy_server) to disable. Called
 * with the lock held.
 */
#define TRACE_PATH "/tmp/cs8210-mdio-trace.txt"
static int trace_fd = -2;

static void trace(uint8_t phy, uint16_t reg, uint16_t val, int is_read, int ret)
{
	char buf[64];
	int n;

	if (trace_fd == -2)
		trace_fd = open(TRACE_PATH, O_WRONLY | O_APPEND | O_CLOEXEC);
	if (trace_fd < 0)
		return;
	n = snprintf(buf, sizeof(buf), "%lld %c %02x %04x %04x %d\n", now_ns(),
		     is_read ? 'R' : 'W', phy, reg, val, ret);
	if (write(trace_fd, buf, n) != n) {
		close(trace_fd);
		trace_fd = -1;
	}
}

static int c45_xfer(uint8_t phy, uint16_t reg, uint16_t *val, int is_read)
{
	int master = phy_to_master(phy);
	uint32_t m;
	int ret = 0;

	if (master < 0) {
		fprintf(stderr, "libMdio: no MDIO master for PHY address 0x%02x\n", phy);
		return -1;
	}
	pthread_mutex_lock(&lock);
	if (fpga_map()) {
		pthread_mutex_unlock(&lock);
		return -1;
	}
	refresh_div(master, phy);
	m = MASTER_BASE(master);
	w16(m + M_ADDR, DEVAD | (phy << 5));
	w16(m + M_REG, reg);
	if (!is_read)
		w16(m + M_DATA, *val);
	w16(m + M_CTRL, (mdc_div[master] << 8) | CTRL_ENABLE | CTRL_START | (is_read ? CTRL_READ : 0));

	if (master_wait(master)) {
		fprintf(stderr, "mdio-%d %s phy:%02x reg:0x%04x timeout\n", master, is_read ? "read" : "write", phy, reg);
		ret = -1;
	} else if (r16(m + M_CTRL) & CTRL_ERROR) {
		fprintf(stderr, "mdio-%d %s phy:%02x reg:0x%04x error!!\n", master, is_read ? "read" : "write", phy, reg);
		ret = -1;
	} else if (is_read) {
		*val = r16(m + M_DATA);
	}
	trace(phy, reg, *val, is_read, ret);
	pthread_mutex_unlock(&lock);
	return ret;
}

/* --- vendor ABI ---------------------------------------------------------- */

int mdio_read_ioctl(uint8_t phy, uint16_t reg, uint16_t *val)
{
	return c45_xfer(phy, reg, val, 1);
}

int mdio_write_ioctl(uint8_t phy, uint16_t reg, uint16_t val)
{
	return c45_xfer(phy, reg, &val, 0);
}

int mdio_read_sysfs(uint8_t phy, uint16_t reg, uint16_t *val)
{
	return c45_xfer(phy, reg, val, 1);
}

int mdio_write_sysfs(uint8_t phy, uint16_t reg, uint16_t val)
{
	return c45_xfer(phy, reg, &val, 0);
}

int mdc_freq_set(uint8_t phy, int hz)
{
	int master = phy_to_master(phy);
	char path[96], buf[24];
	int fd, n;

	if (master < 0)
		return -1;
	if (!hz_to_div(hz, 0)) {
		fprintf(stderr, "libMdio: unsupported MDC frequency %d Hz\n", hz);
		return -1;
	}
	pthread_mutex_lock(&lock);
	if (fpga_map()) {
		pthread_mutex_unlock(&lock);
		return -1;
	}
	mdc_div[master] = hz_to_div(hz, mdc_div[master]);
	pthread_mutex_unlock(&lock);

	/* Keep the launcher's mdc_freq file in step, if it exists. */
	snprintf(path, sizeof(path), "/sys/bus/mdio_bus/devices/mdio-%d:%02x/mdc_freq", master, phy);
	fd = open(path, O_WRONLY | O_TRUNC);
	if (fd >= 0) {
		n = snprintf(buf, sizeof(buf), "%d\n", hz);
		if (write(fd, buf, n) != n)
			fprintf(stderr, "libMdio: updating %s failed\n", path);
		close(fd);
	}
	return 0;
}

static uint8_t slice_to_phy(uint8_t chip, uint8_t sub)
{
	return 2 + chip * 2 + sub;
}

int PHY_mdio_read(uint8_t chip, uint8_t sub, uint16_t reg, uint16_t *val)
{
	int ret = c45_xfer(slice_to_phy(chip, sub), reg, val, 1);

	if (ret)
		puts("PHY_mdio_read driver error");
	return ret;
}

int PHY_mdio_write(uint8_t chip, uint8_t sub, uint16_t reg, uint16_t val)
{
	int ret = c45_xfer(slice_to_phy(chip, sub), reg, &val, 0);

	if (ret)
		puts("PHY_mdio_write driver error");
	return ret;
}

int PHY_mdc_freq_set(uint8_t chip, uint8_t sub, int hz)
{
	int ret = mdc_freq_set(slice_to_phy(chip, sub), hz);

	if (ret)
		puts("PHY_mdc_freq_set driver error");
	return ret;
}

int PHY_mdio_init(void)
{
	uint16_t id = 0;

	/* The vendor library probes PHY 2 for the Credo ID to pick a transport. */
	if (c45_xfer(2, 0x00cc, &id, 1) == 0 && id == 0x2161)
		printf("mdio access: userspace FPGA driver, FPGA version %08x\n", r32(REG_VERSION));
	else
		printf("mdio access: Credo PHY not found at mdio-1:02 (read 0x%04x)\n", id);
	return 0;
}
