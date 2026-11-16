// SPDX-License-Identifier: GPL-2.0+
/*
 * Test-only command for the sandbox write-window test (never in a board
 * build): drive the read-only SPI NOR (+ write window) the way code would.
 *   sfwtest erase <off> <len>          spi_flash_erase()     (the env path)
 *   sfwtest write <off> <len> <byte>   spi_flash_write()
 *   sfwtest op <opcode> <addr bytes> <addr> <dummy bytes> <in|out|none> <len>
 *                                       raw spi_mem_exec_op() on the flash
 * Each prints "sfwtest: rc <n>" for the script to match.
 */
#include <command.h>
#include <dm.h>
#include <malloc.h>
#include <spi.h>
#include <spi-mem.h>
#include <spi_flash.h>
#include <vsprintf.h>
#include <linux/string.h>

static int do_sfwtest(struct cmd_tbl *cmdtp, int flag, int argc, char *const argv[])
{
	struct udevice *dev;
	struct spi_flash *flash;
	int rc;

	if (argc < 2)
		return CMD_RET_USAGE;
	rc = spi_flash_probe_bus_cs(0, 0, &dev);
	if (rc) {
		printf("sfwtest: probe %d\n", rc);
		return CMD_RET_FAILURE;
	}
	flash = dev_get_uclass_priv(dev);
	if (!strcmp(argv[1], "erase") && argc == 4) {
		rc = spi_flash_erase(flash, hextoul(argv[2], NULL), hextoul(argv[3], NULL));
	} else if (!strcmp(argv[1], "write") && argc == 5) {
		ulong len = hextoul(argv[3], NULL);
		u8 *buf = malloc(len);

		memset(buf, hextoul(argv[4], NULL), len);
		rc = spi_flash_write(flash, hextoul(argv[2], NULL), len, buf);
		free(buf);
	} else if (!strcmp(argv[1], "op") && argc == 8) {
		u8 buf[16] = { 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5 };
		ulong n = hextoul(argv[7], NULL);
		struct spi_mem_op op = SPI_MEM_OP(
			SPI_MEM_OP_CMD(hextoul(argv[2], NULL), 1),
			SPI_MEM_OP_ADDR(hextoul(argv[3], NULL), hextoul(argv[4], NULL), 1),
			SPI_MEM_OP_DUMMY(hextoul(argv[5], NULL), 1),
			SPI_MEM_OP_NO_DATA);

		if (n > sizeof(buf))
			return CMD_RET_USAGE;
		if (!strcmp(argv[6], "out")) {
			op.data.dir = SPI_MEM_DATA_OUT;
			op.data.buf.out = buf;
		} else if (!strcmp(argv[6], "in")) {
			op.data.dir = SPI_MEM_DATA_IN;
			op.data.buf.in = buf;
		}
		op.data.nbytes = n;
		op.data.buswidth = n ? 1 : 0;
		rc = spi_mem_exec_op(flash->spi, &op);
	} else {
		return CMD_RET_USAGE;
	}
	printf("sfwtest: rc %d\n", rc);
	return 0;
}

U_BOOT_CMD(sfwtest, 8, 0, do_sfwtest, "SPI NOR write-window test (sandbox only)",
	   "erase <off> <len> | write <off> <len> <byte> | op <opc> <an> <addr> <dummy> <dir> <len>");
