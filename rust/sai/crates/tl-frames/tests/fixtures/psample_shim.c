// SPDX-License-Identifier: Apache-2.0
// LD_PRELOAD shim for the differential test of sflow-probe's psample mode: the kernel has no
// psample family inside the test's network namespace, so a generic-netlink socket is replaced by
// a unix datagram socket pair. The shim answers the CTRL_CMD_GETFAMILY request for "psample" with
// a canned family (id 55, multicast group "packets" = 101) and, when the tool joins the group,
// feeds it psample messages (some malformed or filtered on purpose), the same bytes for the C
// and the Rust tool.
//   SHIM_COUNT  sequence numbers below this are sampled (3, 13, 23, ..)
//   SHIM_IIF    input interface index in the messages
//   SHIM_SIZE   the sent frame size (the sample's original size)
//   SHIM_RATE   sample rate
#define _GNU_SOURCE
#include <dlfcn.h>
#include <linux/genetlink.h>
#include <linux/netlink.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define FAMILY 55
#define GROUP_PACKETS 101

static int fake_fd = -1, peer_fd = -1;

static uint8_t* attr(uint8_t* p, uint16_t type, const void* v, size_t n)
{
    uint16_t len = (uint16_t)(4 + n);
    memcpy(p, &len, 2);
    memcpy(p + 2, &type, 2);
    memcpy(p + 4, v, n);
    size_t pad = ((n + 3) & ~3u) - n;
    memset(p + 4 + n, 0, pad);
    return p + 4 + n + pad;
}

static size_t nlmsg(uint8_t* out, uint16_t type, uint8_t cmd, const uint8_t* body, size_t blen)
{
    struct nlmsghdr h;
    memset(&h, 0, sizeof(h));
    h.nlmsg_len = (uint32_t)(16 + 4 + blen);
    h.nlmsg_type = type;
    h.nlmsg_seq = 1;
    memcpy(out, &h, 16);
    out[16] = cmd;
    out[17] = 1;
    out[18] = out[19] = 0;
    memcpy(out + 20, body, blen);
    return h.nlmsg_len;
}

static void family_reply(void)
{
    uint8_t body[512], *p = body, grp[128], *g, one[64], two[64], *q;
    uint16_t fam = FAMILY;
    p = attr(p, CTRL_ATTR_FAMILY_ID, &fam, 2);
    p = attr(p, CTRL_ATTR_FAMILY_NAME, "psample", 8);
    uint32_t id1 = 100, id2 = GROUP_PACKETS;
    q = attr(one, CTRL_ATTR_MCAST_GRP_NAME, "config", 7);
    q = attr(q, CTRL_ATTR_MCAST_GRP_ID, &id1, 4);
    size_t n1 = (size_t)(q - one);
    q = attr(two, CTRL_ATTR_MCAST_GRP_NAME, "packets", 8);
    q = attr(q, CTRL_ATTR_MCAST_GRP_ID, &id2, 4);
    size_t n2 = (size_t)(q - two);
    g = attr(grp, 1, one, n1);
    g = attr(g, 2, two, n2);
    p = attr(p, CTRL_ATTR_MCAST_GROUPS | NLA_F_NESTED, grp, (size_t)(g - grp));
    uint8_t msg[640];
    size_t n = nlmsg(msg, GENL_ID_CTRL, CTRL_CMD_NEWFAMILY, body, (size_t)(p - body));
    write(peer_fd, msg, n);
}

// one psample message body
static size_t sample_body(uint8_t* b, unsigned seq, unsigned gseq, unsigned group, unsigned orig, unsigned rate, int with_data)
{
    uint8_t* p = b;
    uint16_t iif = (uint16_t)atoi(getenv("SHIM_IIF")), oif = 0, proto = 0x0800;
    p = attr(p, 0, &iif, 2);
    p = attr(p, 1, &oif, 2);
    p = attr(p, 2, &orig, 4);
    p = attr(p, 3, &group, 4);
    p = attr(p, 4, &gseq, 4);
    p = attr(p, 5, &rate, 4);
    p = attr(p, 14, &proto, 2);
    if (with_data) {
        uint8_t f[128];
        memset(f, 0, sizeof(f));
        f[12] = 8;
        f[14] = 0x45;
        memcpy(f + 42, "SFLOWPRB", 8);
        memcpy(f + 50, &seq, 4);
        p = attr(p, 6, f, sizeof(f));
    }
    return (size_t)(p - b);
}

static void* feeder(void* arg)
{
    (void)arg;
    usleep(300000);
    unsigned count = (unsigned)atoi(getenv("SHIM_COUNT")), size = (unsigned)atoi(getenv("SHIM_SIZE")), rate = (unsigned)atoi(getenv("SHIM_RATE"));
    unsigned gseq = 0, k = 0;
    uint8_t body[512], msg[1024], dgram[4096];
    for (unsigned seq = 3; seq < count; seq += 10, k++) {
        unsigned group = k % 4 == 3 ? 2 : 1;
        unsigned orig = k % 7 == 6 ? size + 4 : size;
        unsigned r = k % 5 == 4 ? rate + 1 : rate;
        size_t bl = sample_body(body, seq, ++gseq, group, orig, r, k % 9 != 8);
        size_t n = nlmsg(msg, 55, 0, body, bl);
        if (k % 6 == 5) {   // two messages in one datagram, the second one a duplicate sequence number
            size_t pad = (n + 3) & ~3u;
            memcpy(dgram, msg, n);
            memset(dgram + n, 0, pad - n);
            size_t bl2 = sample_body(body, seq, ++gseq, group, orig, r, 1);
            size_t n2 = nlmsg(dgram + pad, 55, 0, body, bl2);
            write(peer_fd, dgram, pad + n2);
        } else {
            write(peer_fd, msg, n);
        }
        if (k == 2) {   // a control message and an error message are skipped
            size_t n3 = nlmsg(msg, NLMSG_ERROR, 0, body, 4);
            write(peer_fd, msg, n3);
            n3 = nlmsg(msg, 3, 0, body, 4);
            write(peer_fd, msg, n3);
        }
        usleep(500);
    }
    return NULL;
}

int socket(int domain, int type, int protocol)
{
    int (*real)(int, int, int) = dlsym(RTLD_NEXT, "socket");
    if (domain == AF_NETLINK && protocol == NETLINK_GENERIC) {
        int sv[2];
        if (socketpair(AF_UNIX, SOCK_DGRAM, 0, sv) != 0) return -1;
        fake_fd = sv[0];
        peer_fd = sv[1];
        return fake_fd;
    }
    return real(domain, type, protocol);
}

int bind(int fd, const struct sockaddr* a, socklen_t l)
{
    int (*real)(int, const struct sockaddr*, socklen_t) = dlsym(RTLD_NEXT, "bind");
    if (fd == fake_fd) return 0;
    return real(fd, a, l);
}

int setsockopt(int fd, int level, int name, const void* v, socklen_t l)
{
    int (*real)(int, int, int, const void*, socklen_t) = dlsym(RTLD_NEXT, "setsockopt");
    if (fd == fake_fd && level == SOL_NETLINK) {
        if (name == NETLINK_ADD_MEMBERSHIP && *(const uint32_t*)v == GROUP_PACKETS) {
            pthread_t t;
            pthread_create(&t, NULL, feeder, NULL);
            pthread_detach(t);
        }
        return 0;
    }
    return real(fd, level, name, v, l);
}

ssize_t send(int fd, const void* buf, size_t len, int flags)
{
    ssize_t (*real)(int, const void*, size_t, int) = dlsym(RTLD_NEXT, "send");
    if (fd == fake_fd) {
        family_reply();
        return (ssize_t)len;
    }
    return real(fd, buf, len, flags);
}
