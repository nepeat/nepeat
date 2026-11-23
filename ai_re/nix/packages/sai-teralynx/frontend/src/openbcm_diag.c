/* SPDX-License-Identifier: Apache-2.0
 * openbcm-diag: run one Broadcom SDK diag-shell command inside syncd (which
 * owns the chip) through sai-openbcm's SAI_OPENBCM_DIAG_SOCKET.
 *   openbcm-diag [-s SOCKET] COMMAND...
 * Default socket: $OPENBCM_DIAG_SOCKET, else /run/sai-openbcm/diag.sock.
 * Root only (the server checks the peer uid). Exit 0 when connected, 1 when
 * the server refused or the command failed, 2 when there is no server. */
#define _GNU_SOURCE   /* memmem */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

int main(int argc, char** argv)
{
    const char* path = getenv("OPENBCM_DIAG_SOCKET");
    char cmd[1024] = "";
    char buf[4096];
    struct sockaddr_un a;
    size_t len = 0;
    int i = 1, fd, failed = 0;
    ssize_t n;
    if (!path || !*path) path = "/run/sai-openbcm/diag.sock";
    if (argc > 2 && strcmp(argv[1], "-s") == 0) {
        path = argv[2];
        i = 3;
    }
    if (i >= argc) {
        fprintf(stderr, "usage: %s [-s SOCKET] COMMAND...   e.g. %s l2 station show\n", argv[0], argv[0]);
        return 2;
    }
    for (; i < argc; i++) {
        size_t l = strlen(argv[i]);
        if (len + l + 2 >= sizeof(cmd)) {
            fprintf(stderr, "openbcm-diag: command too long\n");
            return 2;
        }
        if (len) cmd[len++] = ' ';
        memcpy(cmd + len, argv[i], l);
        len += l;
    }
    cmd[len++] = '\n';
    memset(&a, 0, sizeof(a));
    a.sun_family = AF_UNIX;
    if (strlen(path) >= sizeof(a.sun_path)) return 2;
    strcpy(a.sun_path, path);
    fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0 || connect(fd, (struct sockaddr*)&a, sizeof(a)) != 0) {
        fprintf(stderr, "openbcm-diag: %s: no syncd diag socket (SAI_OPENBCM_DIAG_SOCKET set? syncd running?)\n", path);
        return 2;
    }
    if (write(fd, cmd, len) != (ssize_t)len) return 2;
    shutdown(fd, SHUT_WR);
    while ((n = read(fd, buf, sizeof(buf))) > 0) {
        if (memmem(buf, (size_t)n, "openbcm-diag: ", 14)) failed = 1;
        fwrite(buf, 1, (size_t)n, stdout);
    }
    close(fd);
    return failed;
}
