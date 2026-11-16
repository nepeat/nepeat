// SPDX-License-Identifier: GPL-2.0-or-later
// qemu-ipd test supervisor: runs a command with a seccomp filter that hands
// every ioctl to us (SECCOMP_RET_USER_NOTIF) and plays a fake ipd driver on
// the file named by $IPD_FAKE: the guest's ioctls must arrive here with the
// guest's own command, argument and argument pointer. Every other ioctl runs
// normally (FLAG_CONTINUE); an ipd-type ('I') one on another file is logged as
// a LEAK. usage: supervisor LOG -- CMD ARGS...
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <poll.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

static long sc(unsigned op, unsigned fl, void *a) { return syscall(SYS_seccomp, op, fl, a); }

static void send_fd(int s, int fd) {
    char c = 0, buf[CMSG_SPACE(sizeof(int))];
    struct iovec iov = { &c, 1 };
    struct msghdr m = { .msg_iov = &iov, .msg_iovlen = 1, .msg_control = buf, .msg_controllen = sizeof buf };
    struct cmsghdr *h = CMSG_FIRSTHDR(&m);
    h->cmsg_level = SOL_SOCKET; h->cmsg_type = SCM_RIGHTS; h->cmsg_len = CMSG_LEN(sizeof(int));
    memcpy(CMSG_DATA(h), &fd, sizeof(int));
    if (sendmsg(s, &m, 0) != 1) _exit(126);
}
static int recv_fd(int s) {
    char c, buf[CMSG_SPACE(sizeof(int))];
    struct iovec iov = { &c, 1 };
    struct msghdr m = { .msg_iov = &iov, .msg_iovlen = 1, .msg_control = buf, .msg_controllen = sizeof buf };
    int fd;
    if (recvmsg(s, &m, 0) != 1) return -1;
    memcpy(&fd, CMSG_DATA(CMSG_FIRSTHDR(&m)), sizeof(int));
    return fd;
}

int main(int argc, char **argv) {
    const char *fake = getenv("IPD_FAKE");
    if (argc < 4 || strcmp(argv[2], "--") || !fake) { fprintf(stderr, "usage: IPD_FAKE=f supervisor LOG -- CMD...\n"); return 2; }
    FILE *log = fopen(argv[1], "w");
    int sv[2];
    socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    pid_t pid = fork();
    if (pid == 0) {
        struct sock_filter f[] = {
            BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
            BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_ioctl, 0, 1),
            BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_USER_NOTIF),
            BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        };
        struct sock_fprog prog = { sizeof f / sizeof f[0], f };
        if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)) _exit(125);
        int nfd = sc(SECCOMP_SET_MODE_FILTER, SECCOMP_FILTER_FLAG_NEW_LISTENER, &prog);
        if (nfd < 0) { perror("seccomp"); _exit(124); }
        send_fd(sv[0], nfd);
        close(nfd); close(sv[0]); close(sv[1]);
        execvp(argv[3], argv + 3);
        _exit(127);
    }
    close(sv[0]);
    int nfd = recv_fd(sv[1]);
    struct seccomp_notif_sizes sz;
    sc(SECCOMP_GET_NOTIF_SIZES, 0, &sz);
    struct seccomp_notif *req = calloc(1, sz.seccomp_notif);
    struct seccomp_notif_resp *resp = calloc(1, sz.seccomp_notif_resp);
    for (;;) {
        struct pollfd p = { nfd, POLLIN, 0 };
        if (poll(&p, 1, -1) < 0 && errno != EINTR) break;
        if (p.revents & POLLHUP) break;
        if (!(p.revents & POLLIN)) continue;
        memset(req, 0, sz.seccomp_notif);
        if (ioctl(nfd, SECCOMP_IOCTL_NOTIF_RECV, req)) { if (errno == EINTR || errno == ENOENT) continue; break; }
        int fd = (int)req->data.args[0];
        unsigned cmd = (unsigned)req->data.args[1];
        uint64_t arg = req->data.args[2];
        char lp[64], path[4096] = "";
        snprintf(lp, sizeof lp, "/proc/%d/fd/%d", req->pid, fd);
        ssize_t n = readlink(lp, path, sizeof path - 1);
        if (n > 0) path[n] = 0;
        memset(resp, 0, sz.seccomp_notif_resp);
        resp->id = req->id;
        if (strcmp(path, fake)) {
            if (((cmd >> 8) & 0xff) == 'I') fprintf(log, "LEAK %s cmd 0x%08x\n", path, cmd);
            resp->flags = SECCOMP_USER_NOTIF_FLAG_CONTINUE;
        } else {
            char mp[64], buf[16];
            snprintf(mp, sizeof mp, "/proc/%d/mem", req->pid);
            int mem = open(mp, O_RDWR);
            fprintf(log, "IPD cmd 0x%08x arg 0x%llx\n", cmd, (unsigned long long)arg);
            switch (cmd) {
            case 0xc0084902:            /* IPD_QUERY_NODE: in/out struct */
                if (pread(mem, buf, 16, arg) == 16 && !memcmp(buf, "IPDTEST-QUERY-IN", 16)) {
                    pwrite(mem, "IPDTEST-QUERY-OK", 16, arg);
                    fprintf(log, "QUERY struct ok\n");
                } else {
                    fprintf(log, "QUERY struct BAD\n");
                    resp->error = -EFAULT;
                }
                break;
            case 0x40044923:            /* IPD_IS_USER_CONFIG_DONE: integer arg */
                fprintf(log, "INT arg %llu\n", (unsigned long long)arg);
                resp->val = 3;
                break;
            case 0xc0084904:            /* IPD_RUPT_WAIT: report an error */
                resp->error = -EINVAL;
                break;
            default:
                fprintf(log, "UNEXPECTED cmd 0x%08x\n", cmd);
                resp->error = -ENOSYS;
            }
            if (mem >= 0) close(mem);
        }
        fflush(log);
        if (ioctl(nfd, SECCOMP_IOCTL_NOTIF_SEND, resp) && errno != ENOENT) perror("NOTIF_SEND");
    }
    int st;
    waitpid(pid, &st, 0);
    fclose(log);
    return WIFEXITED(st) ? WEXITSTATUS(st) : 128 + WTERMSIG(st);
}
