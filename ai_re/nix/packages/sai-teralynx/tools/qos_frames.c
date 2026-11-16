// SPDX-License-Identifier: Apache-2.0
// qos-frames: send tagged IPv4/UDP frames with chosen DSCPs out one Linux
// netdev and count them on one or more netdevs (the CS8210 400G loop:
// Ethernet0 = cage 1 <-> cage 16 = Ethernet120). For the QoS / mirror /
// sFlow chip tests of the SAI: classification (queue counters, read by the
// SAI script), shaping (the received rate), mirror and ERSPAN copies (a
// GRE-encapsulated copy is counted apart, its outer header printed once),
// sampled copies (frames beyond the ones sent).
//   qos-frames TX_IF RX_IF[,RX_IF..] COUNT PPS SIZE DMAC SRC_IP DST_IP DSCP[,DSCP..]
// PPS 0: as fast as the socket takes them. Frames cycle through the DSCPs.
// ECN=1|2|3 (environment): the ECN bits of every frame (ECT(1), ECT(0), CE);
// received frames with CE are counted per DSCP (ce<dscp>=). DMAC "pfc":
// IEEE 802.1Qbb PFC frames instead (01:80:c2:00:00:01, 0x8808, opcode
// 0x0101), the DSCP list = the priorities to pause, quanta 0xffff.
// Prints "RESULT if=.. sent=.. rx=.. gre=.. dscpN=.. rx_seconds=.. rx_mbps=..". Root.
#include <arpa/inet.h>
#include <linux/if_packet.h>
#include <net/ethernet.h>
#include <net/if.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define MAXRX 4
#define MAXD 16
static const char TAG[8] = {'Q', 'O', 'S', 'F', 'R', 'A', 'M', 'E'};
static volatile int stop_rx;
static int ndscp;
struct rx {
    const char* name;
    int fd;
    unsigned long ours, gre, other, bytes, per[MAXD], ce[MAXD];
    double first, last;
    int printed;
    char gre_info[200];
};
static struct rx rxs[MAXRX];

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

static void* rx_loop(void* arg)
{
    struct rx* r = arg;
    uint8_t b[10000];
    while (!stop_rx) {
        struct sockaddr_ll from;
        socklen_t fl = sizeof(from);
        ssize_t n = recvfrom(r->fd, b, sizeof(b), 0, (struct sockaddr*)&from, &fl);
        if (n <= 0) continue;
        if (from.sll_pkttype == PACKET_OUTGOING) continue;
        uint8_t* t = memmem(b, (size_t)n, TAG, sizeof(TAG));
        if (!t || t + sizeof(TAG) + 1 > b + n) { r->other++; continue; }
        double ts = now();
        if (!r->ours && !r->gre) r->first = ts;
        r->last = ts;
        r->bytes += (unsigned long)n;
        // outer IPv4 GRE (protocol 47) in front of our frame: an ERSPAN copy
        if (n > 38 && b[12] == 0x08 && b[13] == 0x00 && b[14 + 9] == 47 && t > b + 34 + 4) {
            r->gre++;
            if (!r->printed) {
                r->printed = 1;
                const uint8_t* ip = b + 14;
                const uint8_t* g = ip + (ip[0] & 15) * 4;
                snprintf(r->gre_info, sizeof(r->gre_info),
                         "outer %u.%u.%u.%u > %u.%u.%u.%u tos=0x%02x ttl=%u gre_flags=0x%02x%02x gre_proto=0x%02x%02x len=%zd",
                         ip[12], ip[13], ip[14], ip[15], ip[16], ip[17], ip[18], ip[19], ip[1], ip[8], g[0], g[1], g[2], g[3], n);
            }
            continue;
        }
        r->ours++;
        int d = t[sizeof(TAG)];
        if (d >= 0 && d < MAXD) {
            r->per[d]++;
            if (n > 16 && b[12] == 0x08 && b[13] == 0x00 && (b[15] & 3) == 3) r->ce[d]++;
        }
    }
    return NULL;
}

static uint16_t csum(const void* p, size_t n)
{
    const uint8_t* b = p;
    uint32_t s = 0;
    for (size_t i = 0; i + 1 < n; i += 2) s += (uint32_t)(b[i] << 8 | b[i + 1]);
    if (n & 1) s += (uint32_t)(b[n - 1] << 8);
    while (s >> 16) s = (s & 0xffff) + (s >> 16);
    return (uint16_t)~s;
}

int main(int argc, char** argv)
{
    if (argc < 10) {
        fprintf(stderr, "usage: %s TX_IF RX_IF[,RX_IF..] COUNT PPS SIZE DMAC SRC_IP DST_IP DSCP[,DSCP..]\n", argv[0]);
        return 2;
    }
    const char* tx = argv[1];
    unsigned count = (unsigned)atoi(argv[3]);
    double pps = atof(argv[4]);
    size_t size = (size_t)atoi(argv[5]);
    if (size < 64) size = 64;
    if (size > 9000) size = 9000;
    uint8_t dmac[6], txmac[6];
    unsigned x[6];
    int pfc = !strcmp(argv[6], "pfc");
    if (pfc) {
        static const uint8_t pm[6] = {0x01, 0x80, 0xc2, 0x00, 0x00, 0x01};
        memcpy(dmac, pm, 6);
    } else {
        if (sscanf(argv[6], "%x:%x:%x:%x:%x:%x", &x[0], &x[1], &x[2], &x[3], &x[4], &x[5]) != 6) return 2;
        for (int i = 0; i < 6; i++) dmac[i] = (uint8_t)x[i];
    }
    int ecn = getenv("ECN") ? atoi(getenv("ECN")) & 3 : 0;
    struct in_addr sip, dip;
    if (inet_pton(AF_INET, argv[7], &sip) != 1 || inet_pton(AF_INET, argv[8], &dip) != 1) return 2;
    int dscps[MAXD];
    char* dl = strdup(argv[9]);
    for (char* tok = strtok(dl, ","); tok && ndscp < MAXD; tok = strtok(NULL, ",")) dscps[ndscp++] = atoi(tok);
    int nrx = 0;
    char* rl = strdup(argv[2]);
    for (char* tok = strtok(rl, ","); tok && nrx < MAXRX; tok = strtok(NULL, ",")) {
        uint8_t m[6];
        struct rx* r = &rxs[nrx++];
        r->name = tok;
        int ix = up_and_index(tok, m);
        r->fd = socket(AF_PACKET, SOCK_RAW, htons(ETH_P_ALL));
        struct sockaddr_ll a;
        memset(&a, 0, sizeof(a));
        a.sll_family = AF_PACKET;
        a.sll_protocol = htons(ETH_P_ALL);
        a.sll_ifindex = ix;
        if (bind(r->fd, (struct sockaddr*)&a, sizeof(a)) != 0) { perror("bind rx"); return 2; }
        int big = 64 << 20;
        setsockopt(r->fd, SOL_SOCKET, SO_RCVBUFFORCE, &big, sizeof(big));
        struct timeval tv = {0, 100000};
        setsockopt(r->fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    }
    int txi = up_and_index(tx, txmac);
    int txfd = socket(AF_PACKET, SOCK_RAW, 0);
    pthread_t th[MAXRX];
    for (int i = 0; i < nrx; i++) pthread_create(&th[i], NULL, rx_loop, &rxs[i]);
    usleep(200000);
    struct sockaddr_ll to;
    memset(&to, 0, sizeof(to));
    to.sll_family = AF_PACKET;
    to.sll_ifindex = txi;
    to.sll_halen = 6;
    memcpy(to.sll_addr, dmac, 6);
    static uint8_t f[9100];
    unsigned sent = 0, failed = 0;
    double t0 = now();
    for (unsigned i = 0; i < count; i++) {
        int di = (int)(i % (unsigned)ndscp);
        memset(f, 0, size);
        if (pfc) {   // MAC control PFC frame: class-enable vector + 8 quanta
            memcpy(f, dmac, 6);
            memcpy(f + 6, txmac, 6);
            f[12] = 0x88; f[13] = 0x08; f[14] = 0x01; f[15] = 0x01;
            uint16_t cev = 0;
            for (int k = 0; k < ndscp; k++) cev |= (uint16_t)(1u << (dscps[k] & 7));
            f[16] = (uint8_t)(cev >> 8); f[17] = (uint8_t)cev;
            for (int k = 0; k < 8; k++)
                if (cev & (1u << k)) { f[18 + 2 * k] = 0xff; f[19 + 2 * k] = 0xff; }
            memcpy(f + 34, TAG, sizeof(TAG));
            if (sendto(txfd, f, 64, 0, (struct sockaddr*)&to, sizeof(to)) == 64) sent++;
            else failed++;
            if (pps > 0) {
                double due = t0 + (i + 1) / pps;
                while (now() < due) usleep(20);
            }
            continue;
        }
        memcpy(f, dmac, 6);
        memcpy(f + 6, txmac, 6);
        f[12] = 0x08;
        f[13] = 0x00;
        uint8_t* ip = f + 14;
        ip[0] = 0x45;
        ip[1] = (uint8_t)(dscps[di] << 2 | ecn);
        uint16_t tot = htons((uint16_t)(size - 14));
        memcpy(ip + 2, &tot, 2);
        ip[8] = 64;
        ip[9] = 17;
        memcpy(ip + 12, &sip, 4);
        memcpy(ip + 16, &dip, 4);
        uint16_t ic = htons(csum(ip, 20));
        memcpy(ip + 10, &ic, 2);
        uint8_t* udp = ip + 20;
        uint16_t sp = htons(40000), dp = htons(9999), ul = htons((uint16_t)(size - 34));
        memcpy(udp, &sp, 2);
        memcpy(udp + 2, &dp, 2);
        memcpy(udp + 4, &ul, 2);
        memcpy(udp + 8, TAG, sizeof(TAG));
        udp[16] = (uint8_t)di;
        memcpy(udp + 17, &i, 4);
        if (sendto(txfd, f, size, 0, (struct sockaddr*)&to, sizeof(to)) == (ssize_t)size) sent++;
        else failed++;
        if (pps > 0) {
            double due = t0 + (i + 1) / pps;
            while (now() < due) usleep(20);
        }
    }
    double t1 = now();
    usleep(1500000);
    stop_rx = 1;
    for (int i = 0; i < nrx; i++) pthread_join(th[i], NULL);
    printf("SENT tx=%s sent=%u send_errors=%u seconds=%.3f tx_mbps=%.1f size=%zu\n", tx, sent, failed, t1 - t0,
           t1 > t0 ? sent * size * 8.0 / (t1 - t0) / 1e6 : 0.0, size);
    for (int i = 0; i < nrx; i++) {
        struct rx* r = &rxs[i];
        double span = r->last - r->first;
        printf("RESULT if=%s sent=%u rx=%lu gre=%lu other=%lu", r->name, sent, r->ours, r->gre, r->other);
        for (int d = 0; d < ndscp; d++) printf(" dscp%d=%lu", dscps[d], r->per[d]);
        for (int d = 0; d < ndscp; d++)
            if (r->ce[d]) printf(" ce%d=%lu", dscps[d], r->ce[d]);
        printf(" rx_seconds=%.3f rx_mbps=%.1f\n", span, span > 0 ? r->bytes * 8.0 / span / 1e6 : 0.0);
        if (r->printed) printf("GRE if=%s %s\n", r->name, r->gre_info);
    }
    fflush(stdout);
    return 0;
}
