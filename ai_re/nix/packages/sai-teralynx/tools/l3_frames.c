// SPDX-License-Identifier: Apache-2.0
// l3-frames: send tagged IPv4/IPv6 UDP frames out one Linux netdev and
// count what comes back on others, grouped by how the ASIC changed them
// (DMAC, SMAC, TTL / hop limit, VLAN tag, length). For L3 tests of the SAI
// on the CS8210 400G loop (Ethernet0 = cage 1 -> DAC -> cage 16 =
// Ethernet120): a frame sent out one netdev enters the ASIC on the other
// port; a routed copy that leaves on port X re-enters on the far port and,
// not being addressed to the router, is flooded to the CPU, i.e. shows up
// on the far port's netdev with the next hop's DMAC, the router's SMAC and
// TTL - 1.
//   l3-frames tx=IF rx=IF[,IF..] dst=IP [src=IP] dmac=MAC [smac=MAC] [count=N] [pps=R]
//             [ttl=N] [size=BYTES] [flows=N] [dst_n=N] [wait_ms=MS]
// flows: the UDP source port cycles through N values (ECMP hashing);
// dst_n: the destination address cycles through N consecutive addresses.
// size: frame length without FCS (min 64). Brings the netdevs up. Root.
// Prints "RESULT ..." and one "RX ..." line per distinct (netdev, vlan,
// dmac, smac, ttl, len) group, with the number of distinct flows seen.
#include <arpa/inet.h>
#include <linux/if_packet.h>
#include <net/ethernet.h>
#include <net/if.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

static const char TAG[8] = {'L', '3', 'F', 'R', 'A', 'M', 'E', 'S'};
#define MAX_RX 4
#define MAX_GROUPS 64
#define MAX_FLOWS 4096

struct group {
    int rx, vlan, ttl, len;
    uint8_t dmac[6], smac[6];
    unsigned long count;
    uint8_t flows[MAX_FLOWS / 8];
};
static struct group groups[MAX_GROUPS];
static int ngroups;
static unsigned long rx_ours, rx_other, rx_dropped_groups;
static uint32_t run_id;

static double now(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

static int up_and_index(const char* ifn, uint8_t mac[6])
{
    int s = socket(AF_INET, SOCK_DGRAM, 0);
    struct ifreq r;
    memset(&r, 0, sizeof(r));
    snprintf(r.ifr_name, IFNAMSIZ, "%s", ifn);
    if (ioctl(s, SIOCGIFFLAGS, &r) != 0) { perror(ifn); exit(2); }
    if (!(r.ifr_flags & IFF_UP)) {
        r.ifr_flags |= IFF_UP;
        if (ioctl(s, SIOCSIFFLAGS, &r) != 0) { perror("up"); exit(2); }
    }
    memset(&r, 0, sizeof(r));
    snprintf(r.ifr_name, IFNAMSIZ, "%s", ifn);
    if (ioctl(s, SIOCGIFHWADDR, &r) != 0) { perror("hwaddr"); exit(2); }
    memcpy(mac, r.ifr_hwaddr.sa_data, 6);
    close(s);
    return (int)if_nametoindex(ifn);
}

static int parse_mac(const char* s, uint8_t m[6])
{
    unsigned x[6];
    if (sscanf(s, "%x:%x:%x:%x:%x:%x", &x[0], &x[1], &x[2], &x[3], &x[4], &x[5]) != 6) return -1;
    for (int i = 0; i < 6; i++) m[i] = (uint8_t)x[i];
    return 0;
}

static uint32_t sum16(const uint8_t* b, size_t n, uint32_t s)
{
    for (size_t i = 0; i + 1 < n; i += 2) s += (uint32_t)(b[i] << 8 | b[i + 1]);
    if (n & 1) s += (uint32_t)(b[n - 1] << 8);
    return s;
}
static uint16_t fold(uint32_t s)
{
    while (s >> 16) s = (s & 0xffff) + (s >> 16);
    return (uint16_t)~s;
}

struct cfg {
    int v6;
    uint8_t dmac[6], smac[6];
    uint8_t src[16], dst[16];
    int ttl, size;
    unsigned flows, dst_n;
};

// Build frame i into f; returns its length.
static size_t build(const struct cfg* c, uint8_t* f, unsigned i)
{
    size_t l3 = 14, ipl = c->v6 ? 40 : 20, len = (size_t)c->size;
    if (len < 64) len = 64;
    memset(f, 0, len);
    memcpy(f, c->dmac, 6);
    memcpy(f + 6, c->smac, 6);
    f[12] = c->v6 ? 0x86 : 0x08;
    f[13] = c->v6 ? 0xdd : 0x00;
    uint8_t* ip = f + l3;
    uint8_t* udp = ip + ipl;
    size_t udpl = len - l3 - ipl;
    uint8_t dst[16];
    memcpy(dst, c->dst, 16);
    unsigned add = c->dst_n ? i % c->dst_n : 0;
    int last = c->v6 ? 15 : 3;
    for (int b = last; b >= 0 && add; b--) { unsigned v = dst[b] + (add & 0xff); dst[b] = (uint8_t)v; add = (add >> 8) + (v >> 8); }
    uint16_t sport = htons((uint16_t)(10000 + (c->flows ? i % c->flows : 0))), dport = htons(7777);
    memcpy(udp, &sport, 2);
    memcpy(udp + 2, &dport, 2);
    uint16_t ul = htons((uint16_t)udpl);
    memcpy(udp + 4, &ul, 2);
    memcpy(udp + 8, TAG, 8);
    memcpy(udp + 16, &run_id, 4);
    memcpy(udp + 20, &i, 4);
    uint16_t fl = (uint16_t)(c->flows ? i % c->flows : 0);
    memcpy(udp + 24, &fl, 2);
    if (c->v6) {
        ip[0] = 0x60;
        memcpy(ip + 4, &ul, 2);
        ip[6] = 17;
        ip[7] = (uint8_t)c->ttl;
        memcpy(ip + 8, c->src, 16);
        memcpy(ip + 24, dst, 16);
        uint8_t ph[40];
        memset(ph, 0, sizeof(ph));
        memcpy(ph, ip + 8, 32);
        uint32_t pl = htonl((uint32_t)udpl);
        memcpy(ph + 32, &pl, 4);
        ph[39] = 17;
        uint16_t cs = htons(fold(sum16(udp, udpl, sum16(ph, 40, 0))));
        if (!cs) cs = 0xffff;
        memcpy(udp + 6, &cs, 2);
    } else {
        ip[0] = 0x45;
        uint16_t tot = htons((uint16_t)(len - l3));
        memcpy(ip + 2, &tot, 2);
        ip[8] = (uint8_t)c->ttl;
        ip[9] = 17;
        memcpy(ip + 12, c->src, 4);
        memcpy(ip + 16, dst, 4);
        uint16_t ic = htons(fold(sum16(ip, 20, 0)));
        memcpy(ip + 10, &ic, 2);   // UDP checksum 0 (none) for IPv4
    }
    return len;
}

static void account(int rx, const uint8_t* b, size_t n)
{
    size_t o = 12;
    int vlan = -1;
    if (n < 14) { rx_other++; return; }
    if (b[12] == 0x81 && b[13] == 0x00) { vlan = (b[14] & 0x0f) << 8 | b[15]; o = 16; }
    int v6 = b[o] == 0x86 && b[o + 1] == 0xdd, v4 = b[o] == 0x08 && b[o + 1] == 0x00;
    size_t ip = o + 2, udp = ip + (v6 ? 40 : 20);
    if ((!v4 && !v6) || n < udp + 26 || memcmp(b + udp + 8, TAG, 8) || memcmp(b + udp + 16, &run_id, 4)) {
        rx_other++;
        return;
    }
    rx_ours++;
    int ttl = v6 ? b[ip + 7] : b[ip + 8];
    uint16_t fl;
    memcpy(&fl, b + udp + 24, 2);
    int g;
    for (g = 0; g < ngroups; g++)
        if (groups[g].rx == rx && groups[g].vlan == vlan && groups[g].ttl == ttl && groups[g].len == (int)n &&
            !memcmp(groups[g].dmac, b, 6) && !memcmp(groups[g].smac, b + 6, 6))
            break;
    if (g == ngroups) {
        if (ngroups == MAX_GROUPS) { rx_dropped_groups++; return; }
        memset(&groups[g], 0, sizeof(groups[g]));
        groups[g].rx = rx;
        groups[g].vlan = vlan;
        groups[g].ttl = ttl;
        groups[g].len = (int)n;
        memcpy(groups[g].dmac, b, 6);
        memcpy(groups[g].smac, b + 6, 6);
        ngroups++;
    }
    groups[g].count++;
    if (fl < MAX_FLOWS) groups[g].flows[fl / 8] |= (uint8_t)(1u << (fl % 8));
}

static int drain(struct pollfd* p, int n, int timeout_ms)
{
    uint8_t b[16384];
    int got = 0;
    while (poll(p, (nfds_t)n, timeout_ms) > 0) {
        for (int k = 0; k < n; k++) {
            if (!(p[k].revents & POLLIN)) continue;
            struct sockaddr_ll from;
            socklen_t fl = sizeof(from);
            ssize_t m = recvfrom(p[k].fd, b, sizeof(b), MSG_DONTWAIT, (struct sockaddr*)&from, &fl);
            if (m <= 0 || from.sll_pkttype == PACKET_OUTGOING) continue;
            account(k, b, (size_t)m);
            got++;
        }
        timeout_ms = 0;
    }
    return got;
}

static const char* arg(int argc, char** argv, const char* k, const char* def)
{
    size_t kl = strlen(k);
    for (int i = 1; i < argc; i++)
        if (!strncmp(argv[i], k, kl) && argv[i][kl] == '=') return argv[i] + kl + 1;
    return def;
}

int main(int argc, char** argv)
{
    const char *tx = arg(argc, argv, "tx", NULL), *rxs = arg(argc, argv, "rx", NULL), *dst = arg(argc, argv, "dst", NULL);
    const char* dmac = arg(argc, argv, "dmac", NULL);
    if (!tx || !rxs || !dst || !dmac) {
        fprintf(stderr, "usage: %s tx=IF rx=IF[,IF] dst=IP dmac=MAC [src=IP] [smac=MAC] [count=N] [pps=R] [ttl=N] "
                        "[size=N] [flows=N] [dst_n=N] [wait_ms=MS]\n", argv[0]);
        return 2;
    }
    struct cfg c;
    memset(&c, 0, sizeof(c));
    c.v6 = strchr(dst, ':') != NULL;
    const char* src = arg(argc, argv, "src", c.v6 ? "fc00::99" : "10.250.0.99");
    if (inet_pton(c.v6 ? AF_INET6 : AF_INET, dst, c.dst) != 1 || inet_pton(c.v6 ? AF_INET6 : AF_INET, src, c.src) != 1) {
        fprintf(stderr, "bad src/dst (same family)\n");
        return 2;
    }
    if (parse_mac(dmac, c.dmac) != 0) { fprintf(stderr, "bad dmac\n"); return 2; }
    unsigned count = (unsigned)atoi(arg(argc, argv, "count", "100"));
    double pps = atof(arg(argc, argv, "pps", "1000"));
    c.ttl = atoi(arg(argc, argv, "ttl", "64"));
    c.size = atoi(arg(argc, argv, "size", "128"));
    c.flows = (unsigned)atoi(arg(argc, argv, "flows", "1"));
    c.dst_n = (unsigned)atoi(arg(argc, argv, "dst_n", "0"));
    int wait_ms = atoi(arg(argc, argv, "wait_ms", "1500"));
    if (c.size > 9600) c.size = 9600;
    if (c.flows > MAX_FLOWS) c.flows = MAX_FLOWS;
    srand((unsigned)(time(NULL) ^ getpid()));
    run_id = (uint32_t)rand() ^ ((uint32_t)rand() << 16);

    uint8_t txmac[6];
    int txi = up_and_index(tx, txmac);
    memcpy(c.smac, txmac, 6);
    const char* smac = arg(argc, argv, "smac", NULL);
    if (smac && parse_mac(smac, c.smac) != 0) { fprintf(stderr, "bad smac\n"); return 2; }
    struct pollfd p[MAX_RX];
    char names[MAX_RX][IFNAMSIZ];
    int nrx = 0;
    char list[256];
    snprintf(list, sizeof(list), "%s", rxs);
    for (char* t = strtok(list, ","); t && nrx < MAX_RX; t = strtok(NULL, ",")) {
        uint8_t m[6];
        int idx = up_and_index(t, m);
        int fd = socket(AF_PACKET, SOCK_RAW, htons(ETH_P_ALL));
        struct sockaddr_ll a;
        memset(&a, 0, sizeof(a));
        a.sll_family = AF_PACKET;
        a.sll_protocol = htons(ETH_P_ALL);
        a.sll_ifindex = idx;
        if (fd < 0 || bind(fd, (struct sockaddr*)&a, sizeof(a)) != 0) { perror("bind rx"); return 2; }
        int big = 8 << 20;
        setsockopt(fd, SOL_SOCKET, SO_RCVBUF, &big, sizeof(big));
        snprintf(names[nrx], IFNAMSIZ, "%s", t);
        p[nrx].fd = fd;
        p[nrx].events = POLLIN;
        nrx++;
    }
    usleep(200000);
    drain(p, nrx, 0);
    int txfd = socket(AF_PACKET, SOCK_RAW, 0);
    struct sockaddr_ll to;
    memset(&to, 0, sizeof(to));
    to.sll_family = AF_PACKET;
    to.sll_ifindex = txi;
    to.sll_halen = 6;
    memcpy(to.sll_addr, c.dmac, 6);
    static uint8_t f[9700];
    unsigned sent = 0, failed = 0;
    double t0 = now();
    for (unsigned i = 0; i < count; i++) {
        size_t n = build(&c, f, i);
        if (sendto(txfd, f, n, 0, (struct sockaddr*)&to, sizeof(to)) == (ssize_t)n) sent++;
        else failed++;
        if (pps > 0) {
            double due = t0 + (i + 1) / pps;
            while (now() < due) { drain(p, nrx, 0); usleep(20); }
        }
        if ((i & 63) == 0) drain(p, nrx, 0);
    }
    double t1 = now();
    double end = now() + wait_ms / 1000.0;
    while (now() < end) drain(p, nrx, 50);
    printf("RESULT tx=%s dst=%s ttl=%d size=%d flows=%u sent=%u send_errors=%u seconds=%.3f rx_ours=%lu rx_other=%lu%s\n",
           tx, dst, c.ttl, c.size, c.flows, sent, failed, t1 - t0, rx_ours, rx_other,
           rx_dropped_groups ? " (groups overflowed)" : "");
    for (int g = 0; g < ngroups; g++) {
        unsigned nf = 0;
        for (unsigned k = 0; k < MAX_FLOWS; k++) nf += (groups[g].flows[k / 8] >> (k % 8)) & 1;
        char vl[16];
        if (groups[g].vlan < 0) snprintf(vl, sizeof(vl), "-");
        else snprintf(vl, sizeof(vl), "%d", groups[g].vlan);
        const uint8_t *d = groups[g].dmac, *s = groups[g].smac;
        printf("RX if=%s vlan=%s dmac=%02x:%02x:%02x:%02x:%02x:%02x smac=%02x:%02x:%02x:%02x:%02x:%02x ttl=%d len=%d count=%lu flows=%u\n",
               names[groups[g].rx], vl, d[0], d[1], d[2], d[3], d[4], d[5], s[0], s[1], s[2], s[3], s[4], s[5],
               groups[g].ttl, groups[g].len, groups[g].count, nf);
    }
    fflush(stdout);
    return 0;
}
