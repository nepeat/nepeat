// SPDX-License-Identifier: Apache-2.0
// raw-frames: send one given Ethernet frame (hex) COUNT times out one Linux
// netdev and count the copies that come back on another (the CS8210 400G
// loop: Ethernet0 = cage 1 -> DAC -> cage 16 = Ethernet120). For ACL and
// trap tests of frames trap-frames has no kind for (IGMP, MLD, ICMP, DSCP,
// IPv6). The source MAC is replaced by the TX netdev's; a tag ("RAWFRAME",
// a run id, the sequence number) is appended after the given bytes (the
// IP length fields do not cover it) and only tagged frames are counted.
//   raw-frames TX_IF RX_IF HEX COUNT PPS
// Prints "RESULT ... rx_ours=N rx_other=M". Brings both netdevs up. Root.
#include <arpa/inet.h>
#include <errno.h>
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

static const char TAG[8] = {'R', 'A', 'W', 'F', 'R', 'A', 'M', 'E'};
static volatile int stop_rx;
static int rx_fd;
static uint32_t run_id;
static unsigned long rx_ours, rx_other;

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
    (void)arg;
    uint8_t b[10000];
    while (!stop_rx) {
        struct sockaddr_ll from;
        socklen_t fl = sizeof(from);
        ssize_t n = recvfrom(rx_fd, b, sizeof(b), 0, (struct sockaddr*)&from, &fl);
        if (n <= 0) continue;
        if (from.sll_pkttype == PACKET_OUTGOING) continue;
        uint8_t* t = memmem(b, (size_t)n, TAG, sizeof(TAG));
        uint32_t id;
        if (t && t + 12 <= b + n && (memcpy(&id, t + 8, 4), id == run_id)) rx_ours++;
        else rx_other++;
    }
    return NULL;
}

int main(int argc, char** argv)
{
    if (argc < 6) {
        fprintf(stderr, "usage: %s TX_IF RX_IF HEX COUNT PPS\n", argv[0]);
        return 2;
    }
    const char *tx = argv[1], *rx = argv[2], *hex = argv[3];
    unsigned count = (unsigned)atoi(argv[4]);
    double pps = atof(argv[5]);
    uint8_t f[2048];
    size_t n = 0;
    for (const char* p = hex; p[0] && p[1] && n < sizeof(f) - 64; p += 2) {
        unsigned v;
        if (sscanf(p, "%2x", &v) != 1) { fprintf(stderr, "bad hex\n"); return 2; }
        f[n++] = (uint8_t)v;
    }
    if (n < 14) { fprintf(stderr, "frame shorter than an Ethernet header\n"); return 2; }
    uint8_t txmac[6], rxmac[6];
    int txi = up_and_index(tx, txmac), rxi = up_and_index(rx, rxmac);
    memcpy(f + 6, txmac, 6);
    run_id = (uint32_t)getpid() ^ (uint32_t)time(NULL);
    size_t base = n;
    int txfd = socket(AF_PACKET, SOCK_RAW, 0);
    rx_fd = socket(AF_PACKET, SOCK_RAW, htons(ETH_P_ALL));
    struct sockaddr_ll a;
    memset(&a, 0, sizeof(a));
    a.sll_family = AF_PACKET;
    a.sll_protocol = htons(ETH_P_ALL);
    a.sll_ifindex = rxi;
    if (bind(rx_fd, (struct sockaddr*)&a, sizeof(a)) != 0) { perror("bind rx"); return 2; }
    struct timeval tv = {0, 100000};
    setsockopt(rx_fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    pthread_t th;
    pthread_create(&th, NULL, rx_loop, NULL);
    usleep(200000);
    struct sockaddr_ll to;
    memset(&to, 0, sizeof(to));
    to.sll_family = AF_PACKET;
    to.sll_ifindex = txi;
    to.sll_halen = 6;
    memcpy(to.sll_addr, f, 6);
    unsigned sent = 0, failed = 0;
    double t0 = now();
    for (unsigned i = 0; i < count; i++) {
        size_t m = base;
        memcpy(f + m, TAG, sizeof(TAG));
        m += sizeof(TAG);
        memcpy(f + m, &run_id, 4);
        m += 4;
        memcpy(f + m, &i, 4);
        m += 4;
        while (m < 60) f[m++] = 0;
        if (sendto(txfd, f, m, 0, (struct sockaddr*)&to, sizeof(to)) == (ssize_t)m) sent++;
        else failed++;
        if (pps > 0) {
            double due = t0 + (i + 1) / pps;
            while (now() < due) usleep(50);
        }
    }
    double t1 = now();
    usleep(1500000);
    stop_rx = 1;
    pthread_join(th, NULL);
    printf("RESULT raw tx=%s rx=%s len=%zu sent=%u send_errors=%u seconds=%.3f rx_ours=%lu rx_other=%lu\n", tx, rx, base, sent,
           failed, t1 - t0, rx_ours, rx_other);
    fflush(stdout);
    return 0;
}
