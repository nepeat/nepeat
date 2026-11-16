// SPDX-License-Identifier: Apache-2.0
// trap-frames: send test frames out one Linux netdev and count them on
// another (the CS8210 400G loop: Ethernet0 = cage 1 -> DAC -> cage 16 =
// Ethernet120). For trap / CoPP tests of the SAI: what the ASIC sends to
// the CPU shows up on the netdev of the port it entered.
//   trap-frames TX_IF RX_IF KIND COUNT PPS [DMAC] [DST_IP]
// KIND: lldp lacp eapol arp bgp ip2me plain. bgp / ip2me need DMAC (the
// router interface MAC) and DST_IP (a trapped address): TCP SYN to port 179
// (bgp) or 8888 (ip2me). plain: a unicast frame no trap matches (control).
// Frames carry a tag (kind + sequence) so only our own are counted on RX.
// Brings both netdevs up. Prints "RESULT kind=.. sent=.. rx=.. ...". Root.
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

static const char TAG[8] = {'T', 'R', 'A', 'P', 'T', 'E', 'S', 'T'};
static volatile int stop_rx;
static int rx_fd, kind_id;
static unsigned long rx_ours, rx_other, rx_bytes;

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
        void* t = memmem(b, (size_t)n, TAG, sizeof(TAG));
        if (t && ((uint8_t*)t)[8] == kind_id) { rx_ours++; rx_bytes += (unsigned long)n; }
        else rx_other++;
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

static int parse_mac(const char* s, uint8_t m[6])
{
    unsigned x[6];
    if (sscanf(s, "%x:%x:%x:%x:%x:%x", &x[0], &x[1], &x[2], &x[3], &x[4], &x[5]) != 6) return -1;
    for (int i = 0; i < 6; i++) m[i] = (uint8_t)x[i];
    return 0;
}

// Build one frame into f; returns its length.
static size_t build(const char* kind, uint8_t* f, const uint8_t smac[6], const uint8_t dmac_arg[6], uint32_t dip,
                    unsigned seq)
{
    static const uint8_t lldp[6] = {0x01, 0x80, 0xc2, 0x00, 0x00, 0x0e}, slow[6] = {0x01, 0x80, 0xc2, 0x00, 0x00, 0x02},
                         pae[6] = {0x01, 0x80, 0xc2, 0x00, 0x00, 0x03}, bc[6] = {0xff, 0xff, 0xff, 0xff, 0xff, 0xff},
                         uc[6] = {0x02, 0x99, 0x99, 0x99, 0x99, 0x99};
    const uint8_t* d = uc;
    uint16_t et = 0x88b5;
    size_t o = 14;
    memset(f, 0, 128);
    if (!strcmp(kind, "lldp")) { d = lldp; et = 0x88cc; }
    else if (!strcmp(kind, "lacp")) { d = slow; et = 0x8809; f[o++] = 0x01; f[o++] = 0x01; }   // LACP subtype, version
    else if (!strcmp(kind, "eapol")) { d = pae; et = 0x888e; f[o++] = 0x02; f[o++] = 0x01; f[o++] = 0; f[o++] = 0; }
    else if (!strcmp(kind, "arp")) {
        d = bc; et = 0x0806;
        uint8_t arp[28] = {0, 1, 8, 0, 6, 4, 0, 1};
        memcpy(arp + 8, smac, 6);
        arp[14] = 10; arp[15] = 0; arp[16] = 0; arp[17] = 2;      // sender 10.0.0.2
        arp[24] = 10; arp[25] = 0; arp[26] = 0; arp[27] = 1;      // target 10.0.0.1
        memcpy(f + o, arp, 28);
        o += 28;
    } else if (!strcmp(kind, "bgp") || !strcmp(kind, "ip2me")) {
        d = dmac_arg; et = 0x0800;
        uint8_t* ip = f + o;
        ip[0] = 0x45; ip[8] = 64; ip[9] = 6;
        uint32_t sip = htonl(0x0a000002), dipn = htonl(dip);
        memcpy(ip + 12, &sip, 4);
        memcpy(ip + 16, &dipn, 4);
        uint8_t* tcp = ip + 20;
        uint16_t sport = htons(40000), dport = htons(!strcmp(kind, "bgp") ? 179 : 8888);
        memcpy(tcp, &sport, 2);
        memcpy(tcp + 2, &dport, 2);
        tcp[12] = 0x50; tcp[13] = 0x02;                            // SYN
        tcp[14] = 0xff; tcp[15] = 0xff;
        size_t pl = 20 + sizeof(TAG) + 1 + 4;
        uint16_t tot = htons((uint16_t)(20 + pl));
        memcpy(ip + 2, &tot, 2);
        memcpy(tcp + 20, TAG, sizeof(TAG));
        tcp[28] = (uint8_t)kind_id;
        memcpy(tcp + 29, &seq, 4);
        uint16_t ic = htons(csum(ip, 20));
        memcpy(ip + 10, &ic, 2);
        uint8_t ph[12 + 64];   // TCP checksum over the pseudo header
        memcpy(ph, ip + 12, 8);
        ph[8] = 0; ph[9] = 6;
        uint16_t tl = htons((uint16_t)pl);
        memcpy(ph + 10, &tl, 2);
        memcpy(ph + 12, tcp, pl);
        uint16_t tc = htons(csum(ph, 12 + pl));
        memcpy(tcp + 16, &tc, 2);
        o += 20 + pl;
    }
    memcpy(f, d, 6);
    memcpy(f + 6, smac, 6);
    f[12] = (uint8_t)(et >> 8);
    f[13] = (uint8_t)et;
    if (strcmp(kind, "bgp") && strcmp(kind, "ip2me")) {
        memcpy(f + o, TAG, sizeof(TAG));
        o += sizeof(TAG);
        f[o++] = (uint8_t)kind_id;
        memcpy(f + o, &seq, 4);
        o += 4;
    }
    return o < 64 ? 64 : o;   // padded minimum frame (FCS added by the MAC)
}

int main(int argc, char** argv)
{
    if (argc < 6) {
        fprintf(stderr, "usage: %s TX_IF RX_IF KIND COUNT PPS [DMAC] [DST_IP]\n", argv[0]);
        return 2;
    }
    const char *tx = argv[1], *rx = argv[2], *kind = argv[3];
    unsigned count = (unsigned)atoi(argv[4]);
    double pps = atof(argv[5]);
    uint8_t dmac[6] = {0}, txmac[6], rxmac[6];
    uint32_t dip = 0;
    if (argc > 6 && parse_mac(argv[6], dmac) != 0) { fprintf(stderr, "bad DMAC\n"); return 2; }
    if (argc > 7) { struct in_addr a; if (inet_pton(AF_INET, argv[7], &a) != 1) return 2; dip = ntohl(a.s_addr); }
    const char* kinds[] = {"plain", "lldp", "lacp", "eapol", "arp", "bgp", "ip2me"};
    kind_id = -1;
    for (int i = 0; i < 7; i++) if (!strcmp(kind, kinds[i])) kind_id = 0x40 + i;
    if (kind_id < 0) { fprintf(stderr, "unknown kind %s\n", kind); return 2; }
    int txi = up_and_index(tx, txmac), rxi = up_and_index(rx, rxmac);
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
    uint8_t f[256];
    unsigned sent = 0, failed = 0;
    double t0 = now();
    for (unsigned i = 0; i < count; i++) {
        size_t n = build(kind, f, txmac, dmac, dip, i);
        memcpy(to.sll_addr, f, 6);
        if (sendto(txfd, f, n, 0, (struct sockaddr*)&to, sizeof(to)) == (ssize_t)n) sent++;
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
    printf("RESULT kind=%s tx=%s rx=%s sent=%u send_errors=%u seconds=%.3f rate=%.1f/s rx_ours=%lu rx_other=%lu rx_bytes=%lu\n",
           kind, tx, rx, sent, failed, t1 - t0, (t1 - t0) > 0 ? sent / (t1 - t0) : 0.0, rx_ours, rx_other, rx_bytes);
    fflush(stdout);
    return 0;
}
