/* Fake phy_server for the offline shim test: phy_server's main loop
 * (command list, daemon(1, 1), FIFO $T/cls_phy_fifo, done flag), with
 * T = argv[1] instead of the chroot's /tmp. */
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

struct ent { char name[512]; int (*fn)(char *); };
static struct ent *tab[256];
static int ntab;
void register_cmds(struct ent *t, int n) { for (int i = 0; i < n; i++) tab[ntab++] = &t[i]; }
extern struct ent credo_cmds[];

int main(int argc, char **argv)
{
	char p[4][600];
	if (argc < 2)
		return 2;
	snprintf(p[0], 600, "%s/phy_server.pid", argv[1]);
	snprintf(p[1], 600, "%s/cls_phy_fifo", argv[1]);
	snprintf(p[2], 600, "%s/cls_phy_cmd_done", argv[1]);
	register_cmds(credo_cmds, 1);
	if (daemon(1, 1))
		return 1;
	FILE *pf = fopen(p[0], "w");
	fprintf(pf, "%d\n", getpid());
	fclose(pf);
	mkfifo(p[1], 0777);
	for (;;) {
		char buf[1024] = { 0 };
		int fd = open(p[1], O_RDONLY), n = 0, r;
		while (n < 1023 && (r = read(fd, buf + n, 1023 - n)) > 0) {
			n += r;
			if (strchr(buf, '\r') || strchr(buf, '\n'))
				break;
		}
		close(fd);
		buf[strcspn(buf, "\r\n")] = 0;
		if (!strcmp(buf, "exit"))
			return 0;
		char name[512];
		sscanf(buf, "%511s", name);
		int found = 0;
		for (int i = 0; i < ntab; i++) {
			char t[512];
			sscanf(tab[i]->name, "%511s", t);
			if (!strcmp(t, name)) {
				tab[i]->fn(buf);
				found = 1;
				break;
			}
		}
		if (!found)
			fprintf(stderr, "fake: would system(%s)\n", buf);
		mkfifo(p[2], 0777);
	}
}
